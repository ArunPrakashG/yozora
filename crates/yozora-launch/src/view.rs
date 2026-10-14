//! The launcher window: a scrim over the monitor and the panel, laid out like the design.

use std::{
    ops::Range,
    time::{Duration, Instant},
};

use gpui::{
    AnyElement, App, Context, Entity, FocusHandle, Focusable, FontWeight, HighlightStyle, InteractiveElement,
    IntoElement, KeyDownEvent, MouseButton, ObjectFit, ParentElement, Render, Rgba, ScrollHandle, SharedString,
    StatefulInteractiveElement, Styled, StyledImage, StyledText, Subscription, Window, div, img, layer_shell::*,
    prelude::*, px, rgb, rgba,
};
use yozora_ui::{
    Icon, Theme, icon_sized,
    input::{Backspace, InputEvent, TextInput},
    spring::{Spring, SpringConfig},
    theme::RADIUS_ITEM,
};

use crate::{
    app::Core,
    projects, scripts,
    search::{self, Mode, Row, Section, Target, Visual},
    sources, spawn,
};

const WIDTH: f32 = 720.;
/// The panel's top, as a share of the screen's height (190 px at 1080).
const TOP: f32 = 0.176;
const SEARCH_H: f32 = 62.;
const FOOT_H: f32 = 40.;
/// How far the panel drops in from while opening.
const DROP: f32 = 8.;
/// Opacity on the first frame. Starting from zero would make the first frame empty, and the
/// point is that the first frame already shows the launcher.
const FADE_FROM: f32 = 0.45;
const FADE_IN: f32 = 0.12;
const LEAVE: Duration = Duration::from_millis(120);
const REDUCED: Duration = Duration::from_millis(90);
/// Stays below the blur rule's `ignore_alpha` of 0.2, so the scrim is never blurred: blurring
/// the whole screen would cost more than the rest of the frame.
const SCRIM: f32 = 0.18;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Closed,
    Open { since: Instant },
    Leaving { since: Instant },
}

/// The Tab view: what can be done with one result.
struct Actions {
    row: Row,
    items: Vec<(SharedString, &'static str, Act)>,
    sel: usize,
}

#[derive(Clone, Debug, PartialEq)]
enum Act {
    Primary,
    AppAction(usize),
    Pin,
    Hide,
    CloseWindow,
    DeleteClip,
    OpenFolder,
    CopyPath,
    Background,
    OpenTerminal,
    /// Runs a script that asks for confirmation.
    Confirmed,
}

const PLACEHOLDER: &str = "Search apps, windows, files and more";

/// A query given whole, from `open QUERY`, is finished: a bare `@code` becomes the chip, as if a
/// space followed.
fn whole(query: &str) -> String {
    let spaced = format!("{query} ");
    match search::committed_prefix(&spaced) {
        Some((_, "")) => spaced,
        _ => query.to_string(),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Modifier {
    None,
    Shift,
    Alt,
}

pub struct LaunchView {
    core: Entity<Core>,
    pub monitor: String,
    input: Entity<TextInput>,
    focus: FocusHandle,
    /// The whole query, mode prefix included.
    query: String,
    /// A mode prefix shown as a chip in front of the field rather than typed in it.
    chip: Option<&'static str>,
    /// The script whose arguments are being typed, by index into the core's scripts.
    args_for: Option<usize>,
    sections: Vec<Section>,
    sel: usize,
    actions: Option<Actions>,
    phase: Phase,
    drop: Spring,
    last: Instant,
    scroll: ScrollHandle,
    generation: u64,
    error: Option<String>,
    _subs: Vec<Subscription>,
}

impl LaunchView {
    pub fn new(core: Entity<Core>, monitor: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextInput::new(PLACEHOLDER, window, cx).bare(19.));
        let subs = vec![
            cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::Changed => this.requery(cx),
                InputEvent::Submit => this.submit(Modifier::None, window, cx),
                InputEvent::Cancel => this.escape(window, cx),
                InputEvent::Blur => {}
            }),
            cx.observe(&core, |this, core, cx| {
                let generation = core.read(cx).generation;
                if this.is_open() && generation != this.generation {
                    this.requery(cx);
                }
            }),
        ];
        Self {
            core,
            monitor,
            input,
            focus: cx.focus_handle(),
            query: String::new(),
            chip: None,
            args_for: None,
            sections: Vec::new(),
            sel: 0,
            actions: None,
            phase: Phase::Closed,
            drop: Spring::new(1.0, SpringConfig::POPOVER),
            last: Instant::now(),
            scroll: ScrollHandle::new(),
            generation: 0,
            error: None,
            _subs: subs,
        }
    }

    pub fn is_open(&self) -> bool {
        matches!(self.phase, Phase::Open { .. })
    }

    pub fn open(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |i, cx| i.set_text(whole(query), cx));
        self.query = String::from("\0");
        self.chip = None;
        self.leave_args(cx);
        self.actions = None;
        self.error = None;
        self.requery(cx);
        self.phase = Phase::Open { since: Instant::now() };
        self.drop = Spring::new(0.0, SpringConfig::POPOVER);
        self.drop.set_target(1.0);
        self.last = Instant::now();
        window.set_layer(Layer::Overlay);
        window.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        window.set_input_region(None);
        let focus = self.input.focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    pub fn set_query(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.chip = None;
        self.leave_args(cx);
        self.input.update(cx, |i, cx| i.set_text(whole(query), cx));
        self.requery(cx);
        let focus = self.input.focus_handle(cx);
        window.focus(&focus, cx);
    }

    /// Lets go of the keyboard and pointer at once, then fades out unless `animate` is false.
    pub fn close(&mut self, animate: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.phase == Phase::Closed {
            return;
        }
        window.set_keyboard_interactivity(KeyboardInteractivity::None);
        window.set_input_region(Some(&[]));
        if animate && matches!(self.phase, Phase::Open { .. }) {
            self.phase = Phase::Leaving { since: Instant::now() };
            cx.notify();
        } else {
            self.finish(window, cx);
        }
    }

    fn finish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.phase = Phase::Closed;
        // Under everything, so it never blocks direct scanout of a fullscreen game.
        window.set_layer(Layer::Background);
        self.chip = None;
        self.leave_args(cx);
        self.input.update(cx, |i, cx| i.set_text("", cx));
        self.sections.clear();
        self.actions = None;
        cx.notify();
    }

    fn requery(&mut self, cx: &mut Context<Self>) {
        let mut typed = self.input.read(cx).text().to_string();
        if let Some(i) = self.args_for {
            let row = self.core.read(cx).scripts.get(i).map(|sc| search::args_row(i, sc));
            self.sections = row.map(|r| vec![Section { title: "Arguments", rows: vec![r] }]).unwrap_or_default();
            self.sel = 0;
            self.query = typed;
            cx.notify();
            return;
        }
        if self.chip.is_none()
            && let Some((prefix, rest)) = search::committed_prefix(&typed)
        {
            self.chip = Some(prefix);
            typed = rest.to_string();
            self.input.update(cx, |i, cx| i.set_text(typed.clone(), cx));
        }
        let text = match self.chip {
            Some(p) => format!("{p} {typed}"),
            None => typed,
        };
        let changed = text != self.query;
        let started = Instant::now();
        self.sections = self.core.update(cx, |c, cx| c.search(&text, cx));
        self.sections.retain(|s| !s.rows.is_empty());
        self.generation = self.core.read(cx).generation;
        let count = self.count();
        if changed {
            self.sel = 0;
            self.actions = None;
            self.error = None;
            self.scroll.set_offset(gpui::point(px(0.), px(0.)));
        }
        self.sel = self.sel.min(count.saturating_sub(1));
        self.query = text;
        tracing::debug!("{} results in {:?}", count, started.elapsed());
        cx.notify();
    }

    /// Asks for a script's arguments in the field.
    fn enter_args(&mut self, i: usize, cx: &mut Context<Self>) {
        let hint = self.core.read(cx).scripts.get(i).map(|sc| {
            sc.args
                .iter()
                .map(|a| if a.optional { format!("{} (optional)", a.placeholder) } else { a.placeholder.clone() })
                .collect::<Vec<_>>()
                .join(", ")
        });
        self.args_for = Some(i);
        self.chip = None;
        self.actions = None;
        self.error = None;
        self.input.update(cx, |input, cx| {
            input.set_placeholder(hint.unwrap_or_default(), cx);
            input.set_text("", cx);
        });
        self.requery(cx);
    }

    fn leave_args(&mut self, cx: &mut Context<Self>) {
        if self.args_for.take().is_some() {
            self.input.update(cx, |i, cx| i.set_placeholder(PLACEHOLDER, cx));
        }
    }

    /// Nothing typed and no script waiting for arguments: the empty view.
    fn idle(&self) -> bool {
        self.query.trim().is_empty() && self.args_for.is_none()
    }

    fn rows(&self) -> impl Iterator<Item = &Row> {
        self.sections.iter().flat_map(|s| s.rows.iter())
    }

    fn count(&self) -> usize {
        self.sections.iter().map(|s| s.rows.len()).sum()
    }

    fn selected(&self) -> Option<&Row> {
        self.rows().nth(self.sel)
    }

    fn mode(&self) -> Mode {
        search::parse_mode(&self.query).0
    }

    fn move_sel(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(a) = &mut self.actions {
            let n = a.items.len() as isize;
            if n > 0 {
                a.sel = (a.sel as isize + delta).rem_euclid(n) as usize;
            }
        } else {
            let n = self.count() as isize;
            if n == 0 {
                return;
            }
            self.sel = (self.sel as isize + delta).rem_euclid(n) as usize;
            self.scroll_to_sel();
        }
        cx.notify();
    }

    /// Body children are section titles and rows in order; finds the selected row's child.
    fn scroll_to_sel(&self) {
        let mut child = 0;
        let mut row = 0;
        for s in &self.sections {
            if s.title == "Pinned" && self.idle() {
                // Tiles: one header and one grid.
                if self.sel < row + s.rows.len() {
                    self.scroll.scroll_to_item(child + 1);
                    return;
                }
                row += s.rows.len();
                child += 2;
                continue;
            }
            child += 1;
            for _ in &s.rows {
                if row == self.sel {
                    self.scroll.scroll_to_item(child);
                    return;
                }
                row += 1;
                child += 1;
            }
        }
    }

    fn escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.actions.take().is_some() {
            cx.notify();
        } else if self.args_for.is_some() {
            self.leave_args(cx);
            self.input.update(cx, |i, cx| i.set_text("", cx));
            self.query = String::from("\0");
            self.requery(cx);
        } else if !self.query.is_empty() {
            self.chip = None;
            self.leave_args(cx);
            self.input.update(cx, |i, cx| i.set_text("", cx));
            self.requery(cx);
        } else {
            self.close(true, window, cx);
        }
    }

    fn submit(&mut self, m: Modifier, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(a) = &self.actions {
            let Some((_, _, act)) = a.items.get(a.sel).cloned() else { return };
            let row = a.row.clone();
            self.perform(&row, act, window, cx);
            return;
        }
        let Some(row) = self.selected().cloned() else { return };
        let act = match (m, &row.target) {
            (Modifier::Shift, Target::App { index, .. }) => {
                let has = self.core.read(cx).apps.get(*index).is_some_and(|e| !e.app.actions.is_empty());
                if has { Act::AppAction(0) } else { Act::Primary }
            }
            (Modifier::Shift, Target::Shell(_)) => Act::Background,
            (Modifier::Shift, Target::File(_)) => Act::CopyPath,
            (Modifier::Alt, Target::File(_)) => Act::OpenFolder,
            (Modifier::Shift, Target::Project(_)) => Act::OpenTerminal,
            (Modifier::Alt, Target::Project(_)) => Act::OpenFolder,
            (Modifier::Shift, Target::Url(_) | Target::Ssh(_)) => Act::CopyPath,
            _ => Act::Primary,
        };
        self.perform(&row, act, window, cx);
    }

    fn web(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (_, q) = search::parse_mode(&self.query);
        if q.is_empty() {
            return;
        }
        let q = q.to_string();
        self.perform(&web_row(&q), Act::Primary, window, cx);
    }

    /// Does `act` to `row`. Most things close the launcher; pinning and hiding don't.
    fn perform(&mut self, row: &Row, act: Act, window: &mut Window, cx: &mut Context<Self>) {
        let terminal = self.core.read(cx).settings.launch.terminal.clone();
        let result: anyhow::Result<bool> = (|| {
            match (&act, &row.target) {
                (Act::Pin, _) => {
                    if let Some(key) = &row.key {
                        let key = key.clone();
                        self.core.update(cx, |c, cx| {
                            c.usage.toggle_pin(&key);
                            c.save(cx);
                            c.changed(cx);
                        });
                    }
                    self.actions = None;
                    return Ok(false);
                }
                (Act::Hide, _) => {
                    if let Some(key) = &row.key {
                        let key = key.clone();
                        self.core.update(cx, |c, cx| {
                            c.usage.hide(&key);
                            c.save(cx);
                            c.changed(cx);
                        });
                    }
                    self.actions = None;
                    return Ok(false);
                }
                (Act::Primary | Act::AppAction(_), Target::App { index, action }) => {
                    let core = self.core.read(cx);
                    let entry = core.apps.get(*index).ok_or_else(|| anyhow::anyhow!("that app is gone"))?;
                    let action = if let Act::AppAction(a) = act { Some(a) } else { *action };
                    spawn::app(&entry.app, action, &terminal)?;
                }
                (Act::Primary, Target::Window(address)) => {
                    let address = address.clone();
                    // After letting go of the keyboard, so focus lands on the window.
                    cx.background_spawn(async move {
                        if let Err(e) = sources::focus(&address) {
                            tracing::warn!("focusing {address}: {e:#}");
                        }
                    })
                    .detach();
                }
                (Act::CloseWindow, Target::Window(address)) => {
                    let address = address.clone();
                    cx.background_spawn(async move {
                        if let Err(e) = sources::close_window(&address) {
                            tracing::warn!("closing {address}: {e:#}");
                        }
                    })
                    .detach();
                }
                (Act::Primary, Target::Command(i)) => {
                    let core = self.core.read(cx);
                    let c = core.commands.get(*i).ok_or_else(|| anyhow::anyhow!("no such command"))?;
                    spawn::shell(&c.run)?;
                }
                (Act::Primary, Target::Copy(text)) => {
                    let text = text.clone();
                    cx.background_spawn(async move { sources::copy_text(&text) }).detach();
                }
                (Act::Primary, Target::Clip(line)) => {
                    let line = line.clone();
                    cx.background_spawn(async move {
                        if let Err(e) = sources::clip_copy(&line) {
                            tracing::warn!("copying from the clipboard history: {e:#}");
                        }
                    })
                    .detach();
                }
                (Act::DeleteClip, Target::Clip(line)) => {
                    sources::clip_delete(line)?;
                    self.core.update(cx, |c, cx| {
                        c.clips.retain(|x| x.line != *line);
                        c.changed(cx);
                    });
                    self.actions = None;
                    return Ok(false);
                }
                (Act::Primary, Target::File(path)) => spawn::open(&path.to_string_lossy())?,
                (Act::OpenFolder, Target::File(path)) => {
                    let dir = if path.is_dir() { path.as_path() } else { path.parent().unwrap_or(path) };
                    spawn::open(&dir.to_string_lossy())?;
                }
                (Act::CopyPath, Target::File(path)) => {
                    let text = path.to_string_lossy().into_owned();
                    cx.background_spawn(async move { sources::copy_text(&text) }).detach();
                }
                (Act::Primary, Target::Shell(cmd)) => spawn::shell_in_terminal(cmd, &terminal)?,
                (Act::Background, Target::Shell(cmd)) => spawn::shell(cmd)?,
                (Act::Primary | Act::Confirmed, Target::Script(i)) => {
                    let sc = self
                        .core
                        .read(cx)
                        .scripts
                        .get(*i)
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("that script is gone"))?;
                    if !sc.args.is_empty() && self.args_for != Some(*i) {
                        self.enter_args(*i, cx);
                        return Ok(false);
                    }
                    if sc.confirm && act != Act::Confirmed {
                        self.actions = Some(Actions {
                            row: row.clone(),
                            items: vec![(format!("Run {}", sc.title).into(), "↵", Act::Confirmed)],
                            sel: 0,
                        });
                        return Ok(false);
                    }
                    let typed =
                        if self.args_for.is_some() { self.input.read(cx).text().to_string() } else { String::new() };
                    let args = scripts::split_args(&typed, sc.args.len());
                    if let Some(a) =
                        sc.args.iter().zip(&args).find(|(a, v)| !a.optional && v.is_empty()).map(|(a, _)| a)
                    {
                        anyhow::bail!("{} needs a value", a.placeholder);
                    }
                    scripts::run(&sc, &args, &terminal)?;
                }
                (Act::OpenFolder, Target::Script(i)) => {
                    let path = self.core.read(cx).scripts.get(*i).map(|s| s.path.clone()).unwrap_or_default();
                    spawn::open(&path.parent().unwrap_or(&path).to_string_lossy())?;
                }
                (Act::Primary, Target::Ssh(alias)) => {
                    spawn::run("ssh", &spawn::in_terminal(&terminal, vec!["ssh".into(), alias.clone()]), None)?;
                }
                (Act::CopyPath, Target::Ssh(alias)) => {
                    let text = format!("ssh {alias}");
                    cx.background_spawn(async move { sources::copy_text(&text) }).detach();
                }
                (Act::Primary, Target::Project(path)) => {
                    let editor = self.core.read(cx).settings.launch.editor.clone();
                    projects::open(path, &editor, &terminal)?;
                }
                (Act::OpenTerminal, Target::Project(path)) => projects::terminal_in(path, &terminal)?,
                (Act::OpenFolder, Target::Project(path)) => spawn::open(&path.to_string_lossy())?,
                (Act::CopyPath, Target::Project(path)) => {
                    let text = path.to_string_lossy().into_owned();
                    cx.background_spawn(async move { sources::copy_text(&text) }).detach();
                }
                (Act::Primary, Target::Url(url)) => spawn::open(url)?,
                (Act::CopyPath, Target::Url(url)) => {
                    let text = url.clone();
                    cx.background_spawn(async move { sources::copy_text(&text) }).detach();
                }
                (Act::Primary, Target::Web(q)) => {
                    let url = self.core.read(cx).settings.launch.web_search.replace("{}", &encode(q));
                    spawn::open(&url)?;
                }
                _ => return Ok(false),
            }
            Ok(true)
        })();
        match result {
            Ok(true) => {
                if let Some(key) = &row.key {
                    let key = key.clone();
                    self.core.update(cx, |c, cx| c.record(&key, cx));
                }
                self.close(true, window, cx);
            }
            Ok(false) => cx.notify(),
            Err(e) => {
                tracing::warn!("{}: {e:#}", row.title);
                self.error = Some(format!("{e}"));
                cx.notify();
            }
        }
    }

    fn toggle_actions(&mut self, cx: &mut Context<Self>) {
        if self.actions.take().is_some() {
            cx.notify();
            return;
        }
        let Some(row) = self.selected().cloned() else { return };
        let mut items: Vec<(SharedString, &'static str, Act)> = Vec::new();
        let pinned = row.pinned;
        match &row.target {
            Target::App { index, .. } => {
                items.push(("Open".into(), "↵", Act::Primary));
                if let Some(e) = self.core.read(cx).apps.get(*index) {
                    for (i, a) in e.app.actions.iter().enumerate() {
                        items.push((a.name.clone().into(), if i == 0 { "Shift ↵" } else { "" }, Act::AppAction(i)));
                    }
                }
                items.push((if pinned { "Unpin" } else { "Pin to the top" }.into(), "Ctrl P", Act::Pin));
                items.push(("Hide from results".into(), "", Act::Hide));
            }
            Target::Command(_) => {
                items.push(("Run".into(), "↵", Act::Primary));
            }
            Target::Window(_) => {
                items.push(("Focus".into(), "↵", Act::Primary));
                items.push(("Close the window".into(), "", Act::CloseWindow));
            }
            Target::Clip(_) => {
                items.push(("Copy".into(), "↵", Act::Primary));
                items.push(("Delete from history".into(), "", Act::DeleteClip));
            }
            Target::File(_) => {
                items.push(("Open".into(), "↵", Act::Primary));
                items.push(("Open the folder".into(), "Alt ↵", Act::OpenFolder));
                items.push(("Copy the path".into(), "Shift ↵", Act::CopyPath));
            }
            Target::Shell(_) => {
                items.push(("Run in a terminal".into(), "↵", Act::Primary));
                items.push(("Run without a terminal".into(), "Shift ↵", Act::Background));
            }
            Target::Copy(_) => items.push(("Copy".into(), "↵", Act::Primary)),
            Target::Web(_) => items.push(("Search".into(), "↵", Act::Primary)),
            Target::Script(_) => {
                items.push(("Run".into(), "↵", Act::Primary));
                items.push(("Open the script's folder".into(), "", Act::OpenFolder));
            }
            Target::Ssh(_) => {
                items.push(("Connect in a terminal".into(), "↵", Act::Primary));
                items.push(("Copy the ssh command".into(), "Shift ↵", Act::CopyPath));
            }
            Target::Project(_) => {
                items.push(("Open in the editor".into(), "↵", Act::Primary));
                items.push(("Open a terminal here".into(), "Shift ↵", Act::OpenTerminal));
                items.push(("Open the folder".into(), "Alt ↵", Act::OpenFolder));
                items.push(("Copy the path".into(), "", Act::CopyPath));
            }
            Target::Url(_) => {
                items.push(("Open".into(), "↵", Act::Primary));
                items.push(("Copy the link".into(), "Shift ↵", Act::CopyPath));
            }
        }
        self.actions = Some(Actions { row, items, sel: 0 });
        cx.notify();
    }

    fn key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let m = &e.keystroke.modifiers;
        let handled = match e.keystroke.key.as_str() {
            "down" => {
                self.move_sel(1, cx);
                true
            }
            "up" => {
                self.move_sel(-1, cx);
                true
            }
            "n" | "j" if m.control => {
                self.move_sel(1, cx);
                true
            }
            "k" if m.control => {
                self.move_sel(-1, cx);
                true
            }
            "tab" => {
                self.toggle_actions(cx);
                true
            }
            "enter" if m.control => {
                self.web(window, cx);
                true
            }
            "enter" if m.shift => {
                self.submit(Modifier::Shift, window, cx);
                true
            }
            "enter" if m.alt => {
                self.submit(Modifier::Alt, window, cx);
                true
            }
            "p" if m.control => {
                if let Some(row) = self.selected().cloned()
                    && row.key.is_some()
                    && matches!(row.target, Target::App { .. })
                {
                    self.perform(&row, Act::Pin, window, cx);
                }
                true
            }
            _ => false,
        };
        if handled {
            cx.stop_propagation();
        }
    }

    /// Opacity and vertical offset this frame, asking for the next frame while moving.
    fn advance(&mut self, reduced: bool, window: &mut Window, cx: &mut Context<Self>) -> (f32, f32) {
        let now = Instant::now();
        let dt = (now - self.last).min(Duration::from_millis(100));
        self.last = now;
        match self.phase {
            Phase::Closed => (0.0, 0.0),
            Phase::Open { since } => {
                let t = (now - since).as_secs_f32();
                if reduced {
                    let o = (FADE_FROM + t / REDUCED.as_secs_f32()).min(1.0);
                    if o < 1.0 {
                        window.request_animation_frame();
                    }
                    return (o, 0.0);
                }
                let settled = self.drop.step(dt);
                let o = FADE_FROM + (1.0 - FADE_FROM) * (t / FADE_IN).min(1.0);
                if !settled || o < 1.0 {
                    window.request_animation_frame();
                }
                (o, (self.drop.value() - 1.0) * DROP)
            }
            Phase::Leaving { since } => {
                let span = if reduced { REDUCED } else { LEAVE };
                let t = ((now - since).as_secs_f32() / span.as_secs_f32()).min(1.0);
                if t >= 1.0 {
                    cx.defer_in(window, |v, window, cx| {
                        if matches!(v.phase, Phase::Leaving { .. }) {
                            v.finish(window, cx);
                        }
                    });
                    return (0.0, 0.0);
                }
                window.request_animation_frame();
                let eased = t * t;
                (1.0 - eased, if reduced { 0.0 } else { -6.0 * eased })
            }
        }
    }
}

fn web_row(q: &str) -> Row {
    Row {
        title: String::new(),
        hl: Vec::new(),
        sub: String::new(),
        visual: Visual::Glyph(Icon::Globe),
        badge: None,
        pinned: false,
        top: false,
        target: Target::Web(q.to_string()),
        key: None,
    }
}

/// Percent-encodes a query for a URL.
fn encode(q: &str) -> String {
    let mut out = String::with_capacity(q.len() * 3);
    for b in q.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Byte ranges for char indices, merged where they touch.
fn byte_ranges(text: &str, chars: &[usize]) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = Vec::new();
    let mut k = 0;
    for (ci, (bi, ch)) in text.char_indices().enumerate() {
        if k < chars.len() && chars[k] == ci {
            k += 1;
            let r = bi..bi + ch.len_utf8();
            match out.last_mut() {
                Some(last) if last.end == r.start => last.end = r.end,
                _ => out.push(r),
            }
        }
    }
    out
}

fn visual(v: &Visual, size: f32, theme: &Theme) -> AnyElement {
    match v {
        Visual::Image(path) => {
            img(path.as_ref().clone()).size(px(size)).flex_none().object_fit(ObjectFit::Contain).into_any_element()
        }
        Visual::Glyph(icon) => div()
            .size(px(size))
            .flex_none()
            .rounded(px((size * 0.28).round()))
            .bg(if theme.dark { rgb(0x2A3245) } else { rgb(0x3B4864) })
            .flex()
            .items_center()
            .justify_center()
            .child(icon_sized(*icon, (size * 0.56).round()).text_color(rgb(0xF1F5FF)))
            .into_any_element(),
        Visual::Emoji(e) => div()
            .size(px(size))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px((size * 0.72).round()))
            .child(e.clone())
            .into_any_element(),
        Visual::Letter(c) => div()
            .size(px(size))
            .flex_none()
            .rounded(px((size * 0.28).round()))
            .bg(theme.accent_soft)
            .text_color(theme.accent)
            .text_size(px((size * 0.45).round()))
            .font_weight(FontWeight::SEMIBOLD)
            .flex()
            .items_center()
            .justify_center()
            .child(SharedString::from(c.to_string()))
            .into_any_element(),
    }
}

fn kbd(text: &'static str, theme: &Theme) -> impl IntoElement {
    div()
        .px(px(5.))
        .py(px(1.))
        .rounded(px(5.))
        .bg(theme.fill_2)
        .font_family(theme.mono.clone())
        .text_size(px(11.))
        .text_color(theme.ink_2)
        .child(text)
}

fn section_title(title: &'static str, theme: &Theme) -> impl IntoElement {
    div()
        .pt(px(10.))
        .pb(px(6.))
        .px(px(10.))
        .text_size(px(11.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.ink_3)
        .child(title.to_uppercase())
}

impl LaunchView {
    fn row(&self, i: usize, row: &Row, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let selected = i == self.sel;
        let size = if row.top { 44. } else { 36. };
        let title_style = HighlightStyle {
            color: Some(theme.accent.into()),
            font_weight: Some(FontWeight::SEMIBOLD),
            ..Default::default()
        };
        let title = StyledText::new(row.title.clone())
            .with_highlights(byte_ranges(&row.title, &row.hl).into_iter().map(|r| (r, title_style)));
        let mut meta = div().flex().flex_none().items_center().gap(px(8.));
        if row.pinned {
            meta = meta.child(icon_sized(Icon::Star, 14.).text_color(theme.accent));
        }
        if let Some(b) = &row.badge {
            meta = meta.child(
                div()
                    .min_w(px(22.))
                    .h(px(20.))
                    .px(px(6.))
                    .rounded(px(6.))
                    .bg(theme.fill_2)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(11.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.ink_2)
                    .child(SharedString::from(b.clone())),
            );
        }
        if selected {
            meta = meta.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.ink_2)
                    .child(row.target.verb())
                    .child(kbd("↵", theme)),
            );
        }
        let mut el = div()
            .id(("row", i))
            .flex()
            .items_center()
            .gap(px(12.))
            .h(px(if row.top { 62. } else { 50. }))
            .px(px(10.))
            .rounded(px(12.))
            .flex_none()
            .child(visual(&row.visual, size, theme))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .child(
                        div()
                            .text_size(px(if row.top { 15.5 } else { 14. }))
                            .font_weight(if row.top { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
                            .text_color(theme.ink)
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(title),
                    )
                    .when(!row.sub.is_empty(), |d| {
                        d.child(
                            div()
                                .mt(px(2.))
                                .text_size(px(12.))
                                .text_color(theme.ink_2)
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .text_ellipsis()
                                .child(SharedString::from(row.sub.clone())),
                        )
                    }),
            )
            .child(meta)
            .on_mouse_move(cx.listener(move |this, _, _, cx| {
                if this.sel != i && this.actions.is_none() {
                    this.sel = i;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.sel = i;
                this.submit(Modifier::None, window, cx);
            }));
        if selected {
            el = el.bg(theme.fill_3).border_1().border_color(theme.hair_2);
        }
        el.into_any_element()
    }

    fn tiles(&self, start: usize, rows: &[Row], theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let mut grid = div().flex().gap(px(6.)).px(px(4.)).pt(px(4.)).pb(px(6.));
        for (k, row) in rows.iter().enumerate() {
            let i = start + k;
            let selected = i == self.sel;
            let mut tile = div()
                .id(("tile", i))
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(8.))
                .pt(px(12.))
                .pb(px(10.))
                .px(px(4.))
                .rounded(px(14.))
                .text_size(px(12.))
                .text_color(if selected { theme.ink } else { theme.ink_2 })
                .child(visual(&row.visual, 48., theme))
                .child(
                    div()
                        .w_full()
                        .flex()
                        .justify_center()
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(SharedString::from(row.title.clone())),
                )
                .on_mouse_move(cx.listener(move |this, _, _, cx| {
                    if this.sel != i {
                        this.sel = i;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.sel = i;
                    this.submit(Modifier::None, window, cx);
                }));
            if selected {
                tile = tile.bg(theme.fill_2);
            }
            grid = grid.child(tile);
        }
        // Keep six columns when fewer are pinned.
        for k in rows.len()..6 {
            grid = grid.child(div().id(("tile-pad", k)).flex_1());
        }
        grid.into_any_element()
    }

    fn body(&self, theme: &Theme, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out = Vec::new();
        if let Some(a) = &self.actions {
            out.push(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .pt(px(8.))
                    .pb(px(10.))
                    .px(px(10.))
                    .child(visual(&a.row.visual, 28., theme))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.ink)
                            .child(SharedString::from(a.row.title.clone())),
                    )
                    .child(div().text_size(px(12.)).text_color(theme.ink_3).child("Tab to go back"))
                    .into_any_element(),
            );
            for (k, (label, keys, _)) in a.items.iter().enumerate() {
                let selected = k == a.sel;
                let mut el = div()
                    .id(("act", k))
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(42.))
                    .px(px(12.))
                    .rounded(px(10.))
                    .text_size(px(13.5))
                    .text_color(theme.ink)
                    .child(label.clone())
                    .child(
                        div().font_family(theme.mono.clone()).text_size(px(11.)).text_color(theme.ink_3).child(*keys),
                    )
                    .on_mouse_move(cx.listener(move |this, _, _, cx| {
                        if let Some(a) = &mut this.actions
                            && a.sel != k
                        {
                            a.sel = k;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(a) = &mut this.actions {
                            a.sel = k;
                        }
                        this.submit(Modifier::None, window, cx);
                    }));
                if selected {
                    el = el.bg(theme.fill_3).border_1().border_color(theme.hair_2);
                }
                out.push(el.into_any_element());
            }
            return out;
        }

        let empty = self.idle();
        let mut i = 0;
        for s in &self.sections {
            out.push(section_title(s.title, theme).into_any_element());
            if empty && s.title == "Pinned" {
                out.push(self.tiles(i, &s.rows, theme, cx));
                i += s.rows.len();
                continue;
            }
            for r in &s.rows {
                out.push(self.row(i, r, theme, cx));
                i += 1;
            }
        }
        if self.sections.is_empty() && !empty {
            let (mode, q) = search::parse_mode(&self.query);
            let msg = match mode {
                Mode::Calc => "Type a calculation, like 12*(3+4) or 5 km to mi".to_string(),
                Mode::Shell => "Type a command to run in a terminal".to_string(),
                Mode::Web => "Type what to search the web for".to_string(),
                Mode::Ssh if q.is_empty() => "No hosts in ~/.ssh/config".to_string(),
                Mode::Projects if q.is_empty() => "No repositories found; add folders to project_roots".to_string(),
                Mode::Browser if q.is_empty() => "No bookmarks or history found".to_string(),
                _ if q.is_empty() => String::new(),
                _ => format!("Nothing matches “{q}”"),
            };
            if !msg.is_empty() {
                out.push(
                    div()
                        .pt(px(22.))
                        .pb(px(10.))
                        .px(px(12.))
                        .text_size(px(13.))
                        .text_color(theme.ink_2)
                        .child(msg)
                        .into_any_element(),
                );
            }
        }
        if empty {
            let mut chips = div().flex().flex_wrap().gap(px(6.)).pt(px(8.)).pb(px(6.)).px(px(10.));
            for (k, (p, mode)) in search::PREFIXES.iter().enumerate() {
                let prefix = *p;
                chips = chips.child(
                    div()
                        .id(("chip", k))
                        .h(px(28.))
                        .px(px(10.))
                        .rounded(px(8.))
                        .flex()
                        .items_center()
                        .gap(px(7.))
                        .text_size(px(12.5))
                        .text_color(theme.ink_2)
                        .bg(theme.fill)
                        .border_1()
                        .border_color(theme.hair)
                        .hover(|s| s.bg(theme.fill_2).text_color(theme.ink))
                        .child(
                            div()
                                .font_family(theme.mono.clone())
                                .text_size(px(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.accent)
                                .child(prefix),
                        )
                        .child(mode.label())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let q = if prefix.starts_with('@') { format!("{prefix} ") } else { prefix.to_string() };
                            this.set_query(&q, window, cx);
                        })),
                );
            }
            out.push(chips.into_any_element());
        }
        out
    }

    fn footer(&self, theme: &Theme) -> impl IntoElement {
        let empty = self.idle();
        let current = self.selected();
        let left: SharedString = if let Some(e) = &self.error {
            e.clone().into()
        } else if let Some(a) = &self.actions {
            format!("Actions for {}", a.row.title).into()
        } else {
            match current.map(|r| (&r.target, r)) {
                Some((Target::Web(_), _)) => "Search the web".into(),
                Some((Target::Copy(_), _)) => "Copy the result".into(),
                Some((t, r)) => format!("{} {}", t.verb(), r.title.trim_start_matches("= ")).into(),
                None if empty => "Yozora Launch".into(),
                None => "No matches".into(),
            }
        };
        let hints: Vec<(&'static str, &'static str)> = if self.actions.is_some() {
            vec![("↑↓", "Choose"), ("↵", "Run"), ("Tab", "Back")]
        } else if empty {
            vec![("↑↓", "Move"), ("↵", "Open"), ("Ctrl P", "Pin")]
        } else {
            vec![("↵", current.map_or("Open", |r| r.target.verb())), ("Tab", "Actions"), ("Ctrl ↵", "Web")]
        };
        let mut h = div().flex().gap(px(16.));
        for (k, l) in hints {
            h = h.child(div().flex().items_center().gap(px(6.)).child(kbd(k, theme)).child(l));
        }
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .h(px(FOOT_H))
            .px(px(16.))
            .border_t_1()
            .border_color(theme.hair)
            .text_size(px(12.))
            .text_color(if self.error.is_some() { theme.danger } else { theme.ink_2 })
            .child(div().flex_1().min_w_0().whitespace_nowrap().overflow_hidden().text_ellipsis().child(left))
            .child(h.text_color(theme.ink_2))
    }
}

impl Focusable for LaunchView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for LaunchView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.phase == Phase::Closed {
            return div().into_any_element();
        }
        if let Phase::Open { since } = self.phase
            && self.drop.value() == 0.0
        {
            tracing::debug!("first frame {:?} after opening", since.elapsed());
        }
        let theme = Theme::global(cx).clone();
        let (opacity, offset) = self.advance(theme.motion.reduced, window, cx);
        let viewport = window.viewport_size();
        let (vw, vh) = (f32::from(viewport.width), f32::from(viewport.height));
        let width = WIDTH.min(vw - 32.);
        let top = (vh * TOP).round();
        let body_max = (vh - top - SEARCH_H - FOOT_H - 48.).max(160.);
        let mode = self.mode();

        let scrim = if theme.dark { rgba(0x05080F00) } else { rgba(0x0F162600) };
        let scrim = Rgba { a: if theme.dark { SCRIM } else { SCRIM * 0.6 } * opacity, ..scrim };

        let search_icon: AnyElement = if mode == Mode::All {
            icon_sized(Icon::Search, 20.).text_color(theme.ink_2).into_any_element()
        } else {
            div()
                .h(px(28.))
                .px(px(10.))
                .rounded(RADIUS_ITEM)
                .flex()
                .flex_none()
                .items_center()
                .gap(px(7.))
                .text_size(px(12.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.accent)
                .bg(theme.accent_soft)
                .child(icon_sized(mode.icon(), 14.).text_color(theme.accent))
                .child(mode.label())
                .into_any_element()
        };

        let body = self.body(&theme, cx);
        let has_body = !body.is_empty();
        let panel = div()
            .id("panel")
            .absolute()
            .top(px(top + offset))
            .left(px(((vw - width) / 2.).round()))
            .w(px(width))
            .flex()
            .flex_col()
            .rounded(px(20.))
            .bg(theme.glass_pop)
            .border_1()
            .border_color(theme.hair_2)
            .shadow(theme.popover_shadow())
            .overflow_hidden()
            .opacity(opacity)
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(12.))
                    .h(px(SEARCH_H))
                    .px(px(18.))
                    .when(has_body, |d| d.border_b_1().border_color(theme.hair))
                    .child(search_icon)
                    .child(div().flex_1().min_w_0().child(self.input.clone()))
                    .child(
                        div()
                            .flex_none()
                            .px(px(7.))
                            .py(px(3.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(theme.hair_2)
                            .font_family(theme.mono.clone())
                            .text_size(px(11.))
                            .text_color(theme.ink_3)
                            .child("esc"),
                    ),
            )
            .when(has_body, |d| {
                d.child(
                    div()
                        .id("body")
                        .flex()
                        .flex_col()
                        .p(px(8.))
                        .max_h(px(body_max))
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .children(body),
                )
            })
            .child(self.footer(&theme));

        div()
            .id("launch")
            .key_context("YozoraLaunch")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .bg(scrim)
            .font_family(theme.font.clone())
            .text_color(theme.ink)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| this.key(e, window, cx)))
            // Before the field sees it: Backspace in an empty field drops the mode chip.
            .capture_action(cx.listener(|this, _: &Backspace, _, cx| {
                if this.args_for.is_some() && this.input.read(cx).text().is_empty() {
                    this.leave_args(cx);
                    this.query = String::from("\0");
                    this.requery(cx);
                    cx.stop_propagation();
                } else if this.chip.is_some() && this.input.read(cx).text().is_empty() {
                    this.chip = None;
                    this.requery(cx);
                    cx.stop_propagation();
                }
            }))
            // A click outside the panel closes, as Escape does with nothing typed.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    if this.is_open() {
                        this.close(true, window, cx);
                    }
                }),
            )
            .child(panel)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_ranges() {
        assert_eq!(byte_ranges("Firefox", &[4, 5, 6]), vec![4..7]);
        assert_eq!(byte_ranges("Visual Studio Code", &[0, 7, 14]), vec![0..1, 7..8, 14..15]);
        assert_eq!(byte_ranges("Café Noir", &[3, 4, 5]), vec![3..7]);
    }

    #[test]
    fn whole_queries_commit_the_prefix() {
        assert_eq!(whole("@code"), "@code ");
        assert_eq!(whole("@s"), "@s ");
        assert_eq!(whole("@code yoz"), "@code yoz");
        assert_eq!(whole("@codex"), "@codex");
        assert_eq!(whole("firefox"), "firefox");
    }

    #[test]
    fn query_encoding() {
        assert_eq!(encode("rust gpui"), "rust+gpui");
        assert_eq!(encode("a&b=c/é"), "a%26b%3Dc%2F%C3%A9");
    }
}
