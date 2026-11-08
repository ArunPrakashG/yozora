//! The launcher: one line of text over a list of what it finds. Typing, arrows, Enter and Esc are
//! handled here from the stage's key events, so it works the moment it opens. What it finds, and
//! what choosing a row does, is the search engine's; this file is the state the field and the
//! list keep, and the keys that move them. The drawing is in `view`.

mod view;

use std::process::{Command, Stdio};
use std::sync::{Arc, Once};
use std::time::Instant;

use rune::{Key, KeyEvent, Tx};
use yozora_search::{Act, ActionItem, Modifier, Outcome, Row, Section, Target};

use crate::shared::Shared;
use crate::stage::Stage;

pub use view::{mark_request, view, warm_view};

pub const W: f32 = 620.0;

/// The Tab view: what can be done with one result.
pub struct Actions {
    pub row: Row,
    pub items: Vec<ActionItem>,
    pub sel: usize,
}

#[derive(Default)]
pub struct Launcher {
    /// What is typed, without the mode chip.
    pub text: String,
    /// The caret, and the other end of the selection, as byte offsets into `text`.
    pub caret: usize,
    pub anchor: usize,
    /// A mode prefix shown as a chip in front of the field rather than typed in it.
    pub chip: Option<&'static str>,
    /// The script whose arguments are being typed, by index into the engine's scripts.
    pub args_for: Option<usize>,
    pub sections: Vec<Section>,
    /// One element key per row, in order, so a row keeps its identity as the list changes.
    pub keys: Vec<String>,
    pub sel: usize,
    pub actions: Option<Actions>,
    pub error: Option<String>,
    /// What the current query is still waiting on, if anything.
    pub loading: Option<&'static str>,
    generation: u64,
    /// When it opened, for the rows' staggered arrival.
    pub opened: Option<Instant>,
    /// Closed by a click outside: the next opening keeps what was typed and selected.
    kept: bool,
}

impl Launcher {
    /// The whole query, mode prefix included.
    pub fn query(&self) -> String {
        match self.chip {
            Some(p) => format!("{p} {}", self.text),
            None => self.text.clone(),
        }
    }

    /// Nothing typed and no script waiting for arguments: the empty view.
    pub fn idle(&self) -> bool {
        self.args_for.is_none() && self.query().trim().is_empty()
    }

    pub fn rows(&self) -> impl Iterator<Item = &Row> {
        self.sections.iter().flat_map(|s| s.rows.iter())
    }

    pub fn count(&self) -> usize {
        self.sections.iter().map(|s| s.rows.len()).sum()
    }

    pub fn selected(&self) -> Option<&Row> {
        self.rows().nth(self.sel)
    }

    /// The selected text, as byte offsets.
    pub fn selection(&self) -> (usize, usize) {
        (self.caret.min(self.anchor), self.caret.max(self.anchor))
    }

    fn replace_selection(&mut self, with: &str) {
        let (a, b) = self.selection();
        self.text.replace_range(a..b, with);
        self.caret = a + with.len();
        self.anchor = self.caret;
    }

    fn before(&self, i: usize) -> usize {
        self.text[..i].char_indices().next_back().map_or(0, |(k, _)| k)
    }

    fn after(&self, i: usize) -> usize {
        self.text[i..].chars().next().map_or(i, |c| i + c.len_utf8())
    }

    /// The start of the word before `i`: back over spaces, then over the word.
    fn word_before(&self, i: usize) -> usize {
        let head = &self.text[..i];
        let trimmed = head.trim_end().len();
        head[..trimmed].char_indices().rev().find(|(_, c)| c.is_whitespace()).map_or(0, |(k, c)| k + c.len_utf8())
    }

    fn word_after(&self, i: usize) -> usize {
        let tail = &self.text[i..];
        let word = tail.trim_start();
        let skipped = tail.len() - word.len();
        i + skipped + word.find(char::is_whitespace).unwrap_or(word.len())
    }

    fn move_caret(&mut self, to: usize, extend: bool) {
        self.caret = to;
        if !extend {
            self.anchor = to;
        }
    }

    fn set_text(&mut self, text: String) {
        self.caret = text.len();
        self.anchor = self.caret;
        self.text = text;
    }
}

/// A query given whole, from `open QUERY`, is finished: a bare `@code` becomes the chip, as if a
/// space followed.
fn whole(query: &str) -> String {
    let spaced = format!("{query} ");
    match yozora_search::committed_prefix(&spaced) {
        Some((_, "")) => spaced,
        _ => query.to_string(),
    }
}

/// Turns a finished mode prefix at the start of the field into the chip.
fn commit_prefix(l: &mut Launcher) {
    if l.chip.is_some() || l.args_for.is_some() {
        return;
    }
    let rest = yozora_search::committed_prefix(&l.text).map(|(p, rest)| (p, rest.to_string()));
    if let Some((prefix, rest)) = rest {
        l.chip = Some(prefix);
        l.set_text(rest);
    }
}

/// Searches again for what is typed. `reset` is for an edit: the selection goes back to the top
/// and a menu, an error or a half-typed argument line is dropped. Without it the selection stays
/// where it was, for when the engine merely has more.
fn requery(s: &mut Stage, reset: bool) {
    let sh = s.sh.clone();
    let engine = sh.engine();
    let l = &mut s.launcher;
    if let Some(i) = l.args_for {
        l.sections = engine.args_row(i).map(|r| vec![Section { title: "Arguments", rows: vec![r] }]).unwrap_or_default();
        l.loading = None;
        l.sel = 0;
    } else {
        commit_prefix(l);
        let query = l.query();
        l.sections = engine.search(&query);
        l.loading = engine.loading(&query);
    }
    l.generation = engine.generation();
    l.keys = view::keys(&l.sections);
    if reset {
        l.sel = 0;
        l.actions = None;
        l.error = None;
    }
    l.sel = l.sel.min(l.count().saturating_sub(1));
}

pub fn open(s: &mut Stage) {
    let kept = std::mem::take(&mut s.launcher.kept);
    if kept {
        s.launcher.opened = Some(Instant::now());
    } else {
        s.launcher = Launcher { opened: Some(Instant::now()), ..Default::default() };
    }
    // The windows, scripts and hosts change between opens; this returns at once.
    s.sh.engine().refresh();
    requery(s, !kept);
}

/// A click outside closes the launcher but leaves it as it was, for the next opening.
pub fn dismiss(s: &mut Stage) {
    s.launcher.kept = true;
    s.open = None;
}

/// Sets what is typed, from `open QUERY` or a prefix chip: a finished prefix becomes the chip.
pub fn set_query(s: &mut Stage, query: &str) {
    let l = &mut s.launcher;
    l.chip = None;
    l.args_for = None;
    l.set_text(whole(query));
    requery(s, true);
}

/// Searches again if the engine found more since the last draw.
pub fn sync(s: &mut Stage) {
    if s.sh.engine().generation() != s.launcher.generation {
        requery(s, false);
    }
}

/// Everything the first opening would otherwise pay for, done at startup: the engine and its
/// indexes, `launch.toml` and its changes, and the icons of the apps most likely to show.
pub fn warm(sh: &Arc<Shared>) {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let sh = sh.clone();
        let _ = std::thread::Builder::new().name("launcher-warm".into()).spawn(move || {
            let engine = sh.engine();
            watch_settings(sh.clone());
            // Icons wait for the app index, which the engine builds on its own thread.
            for _ in 0..100 {
                if !engine.apps().is_empty() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            for path in engine.warm_icon_paths() {
                view::hold_image(&sh, &path);
            }
            sh.redraw();
        });
    });
}

/// Applies edits to `launch.toml` (and `theme.toml`, which it reads too) as they happen.
fn watch_settings(sh: Arc<Shared>) {
    let _ = std::thread::Builder::new().name("launcher-config".into()).spawn(move || {
        let dir = yozora_config::paths::config_dir();
        let Ok(mut watcher) = yozora_config::watch::DirWatcher::new(&dir) else { return };
        loop {
            match futures_lite::future::block_on(watcher.changed()) {
                Ok(files) if files.iter().any(|f| f == yozora_config::launch::FILE || f == "theme.toml") => {
                    sh.engine().set_settings(yozora_search::load_settings(&dir));
                }
                Ok(_) => {}
                Err(_) => return,
            }
        }
    });
}

/// Asks for a script's arguments in the field.
fn enter_args(s: &mut Stage, script: usize) {
    let l = &mut s.launcher;
    l.args_for = Some(script);
    l.chip = None;
    l.actions = None;
    l.error = None;
    l.set_text(String::new());
    requery(s, true);
}

fn leave_args(l: &mut Launcher) {
    l.args_for = None;
    l.set_text(String::new());
}

/// What Esc does, one step at a time: the menu, then the arguments, then the text, then the
/// launcher.
fn escape(s: &mut Stage) {
    let l = &mut s.launcher;
    if l.actions.take().is_some() {
    } else if l.args_for.is_some() {
        leave_args(l);
        requery(s, true);
    } else if !l.query().is_empty() {
        l.chip = None;
        l.set_text(String::new());
        requery(s, true);
    } else {
        s.open = None;
    }
}

fn step(s: &mut Stage, delta: isize, tx: &mut Tx) {
    let l = &mut s.launcher;
    if let Some(a) = &mut l.actions {
        let n = a.items.len() as isize;
        if n > 0 {
            a.sel = (a.sel as isize + delta).rem_euclid(n) as usize;
        }
        return;
    }
    let n = l.count() as isize;
    if n == 0 {
        return;
    }
    l.sel = (l.sel as isize + delta).rem_euclid(n) as usize;
    reveal(l, tx);
}

/// Scrolls the list to show the selected row, and its section's title when it is the first.
fn reveal(l: &Launcher, tx: &mut Tx) {
    if let Some(k) = l.keys.get(l.sel) {
        tx.scroll_into_view(k.clone());
    }
}

/// Moves the selection to row `i`, as the pointer does.
pub fn point_at(s: &mut Stage, i: usize) {
    if s.launcher.actions.is_none() && s.launcher.sel != i {
        s.launcher.sel = i;
    }
}

pub fn point_at_action(s: &mut Stage, i: usize) {
    if let Some(a) = &mut s.launcher.actions {
        a.sel = i;
    }
}

/// Chooses row `i` with a click.
pub fn choose(s: &mut Stage, i: usize) {
    s.launcher.sel = i;
    submit(s, Modifier::None);
}

pub fn choose_action(s: &mut Stage, i: usize) {
    point_at_action(s, i);
    submit(s, Modifier::None);
}

/// A click on a prefix chip in the empty view.
pub fn pick_prefix(s: &mut Stage, prefix: &str) {
    let q = if prefix.starts_with('@') { format!("{prefix} ") } else { prefix.to_string() };
    set_query(s, &q);
}

fn submit(s: &mut Stage, m: Modifier) {
    if let Some(a) = &s.launcher.actions {
        let Some(item) = a.items.get(a.sel) else { return };
        let (row, act) = (a.row.clone(), item.act);
        perform(s, row, act, true);
        return;
    }
    let Some(row) = s.launcher.selected().cloned() else { return };
    let act = s.sh.engine().default_act(&row, m);
    perform(s, row, act, true);
}

/// Ctrl+Enter: search the web for what is typed, whatever is selected.
fn web(s: &mut Stage) {
    let query = s.launcher.query();
    let (_, q) = yozora_search::parse_mode(&query);
    if q.is_empty() {
        return;
    }
    let row = yozora_search::web_row(q);
    perform(s, row, Act::Primary, false);
}

/// Does `act` to `row`. Most things close the launcher; pinning and hiding don't. `typed` says
/// whether the argument line, if there is one, goes with it.
fn perform(s: &mut Stage, row: Row, act: Act, typed: bool) {
    let sh = s.sh.clone();
    let args = if typed && s.launcher.args_for.is_some() { Some(s.launcher.text.clone()) } else { None };
    match sh.engine().activate(&row, act, args.as_deref()) {
        Ok(Outcome::Launched) => s.open = None,
        Ok(Outcome::Updated) => {
            s.launcher.actions = None;
            requery(s, false);
        }
        Ok(Outcome::NeedArgs(i)) => enter_args(s, i),
        Ok(Outcome::Confirm(item)) => s.launcher.actions = Some(Actions { row, items: vec![item], sel: 0 }),
        Ok(Outcome::Nothing) => {}
        Err(e) => {
            tracing::warn!("{}: {e:#}", row.title);
            s.launcher.error = Some(format!("{e}"));
        }
    }
}

fn toggle_actions(s: &mut Stage) {
    if s.launcher.actions.take().is_some() {
        return;
    }
    let Some(row) = s.launcher.selected().cloned() else { return };
    let items = s.sh.engine().actions(&row);
    s.launcher.actions = Some(Actions { row, items, sel: 0 });
}

/// What the field shows changed: search again from the top.
fn edited(s: &mut Stage, tx: &mut Tx) {
    requery(s, true);
    tx.scroll_to("lscroll", 0.0, 0.0);
}

fn paste() -> String {
    let out = Command::new("wl-paste").arg("-n").stderr(Stdio::null()).output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).replace(['\n', '\r', '\t'], " "),
        _ => String::new(),
    }
}

fn copy(text: String) {
    let _ = std::thread::Builder::new().name("launcher-copy".into()).spawn(move || {
        use std::io::Write;
        let Ok(mut child) = Command::new("wl-copy").stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else { return };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        let _ = child.wait();
    });
}

/// What a key does. Returns whether it was used.
pub fn on_key(s: &mut Stage, e: &KeyEvent, tx: &mut Tx) -> bool {
    let m = e.mods;
    let extend = m.shift;
    match &e.key {
        Key::Escape => escape(s),
        Key::Down => step(s, 1, tx),
        Key::Up => step(s, -1, tx),
        Key::Command('n' | 'j') if m.ctrl => step(s, 1, tx),
        Key::Command('k') if m.ctrl => step(s, -1, tx),
        Key::Tab => toggle_actions(s),
        Key::Enter if m.ctrl => web(s),
        Key::Enter => submit(s, if m.shift { Modifier::Shift } else if m.alt { Modifier::Alt } else { Modifier::None }),
        Key::Command('p') if m.ctrl => {
            if let Some(row) = s.launcher.selected().cloned()
                && row.key.is_some()
                && matches!(row.target, Target::App { .. })
            {
                perform(s, row, Act::Pin, false);
            }
        }
        Key::Command('a') => {
            let l = &mut s.launcher;
            l.anchor = 0;
            l.caret = l.text.len();
        }
        Key::Command('c') | Key::Command('x') => {
            let l = &mut s.launcher;
            let (a, b) = l.selection();
            if a != b {
                copy(l.text[a..b].to_string());
                if e.key == Key::Command('x') {
                    l.replace_selection("");
                    l.actions = None;
                    edited(s, tx);
                }
            }
        }
        Key::Command('v') => {
            let text = paste();
            if !text.is_empty() {
                s.launcher.replace_selection(&text);
                s.launcher.actions = None;
                edited(s, tx);
            }
        }
        Key::Command('u') => {
            let l = &mut s.launcher;
            l.set_text(String::new());
            l.actions = None;
            edited(s, tx);
        }
        Key::Command('w') => {
            let l = &mut s.launcher;
            if l.selection().0 == l.selection().1 {
                let to = l.word_before(l.caret);
                l.anchor = to;
            }
            l.replace_selection("");
            l.actions = None;
            edited(s, tx);
        }
        Key::Backspace => {
            let l = &mut s.launcher;
            if l.text.is_empty() && l.args_for.is_some() {
                // Backspace in an empty argument line goes back to the list.
                l.args_for = None;
            } else if l.text.is_empty() && l.chip.is_some() {
                // Backspace in an empty field drops the mode chip.
                l.chip = None;
            } else {
                if l.selection().0 == l.selection().1 {
                    l.anchor = if m.cmd || m.alt { l.word_before(l.caret) } else { l.before(l.caret) };
                }
                l.replace_selection("");
            }
            l.actions = None;
            edited(s, tx);
        }
        Key::Delete => {
            let l = &mut s.launcher;
            if l.selection().0 == l.selection().1 {
                l.anchor = if m.cmd || m.alt { l.word_after(l.caret) } else { l.after(l.caret) };
            }
            l.replace_selection("");
            l.actions = None;
            edited(s, tx);
        }
        Key::Left => {
            let l = &mut s.launcher;
            let (a, b) = l.selection();
            let to = if !extend && a != b { a } else if m.cmd || m.alt { l.word_before(l.caret) } else { l.before(l.caret) };
            l.move_caret(to, extend);
        }
        Key::Right => {
            let l = &mut s.launcher;
            let (a, b) = l.selection();
            let to = if !extend && a != b { b } else if m.cmd || m.alt { l.word_after(l.caret) } else { l.after(l.caret) };
            l.move_caret(to, extend);
        }
        Key::Home => s.launcher.move_caret(0, extend),
        Key::End => {
            let end = s.launcher.text.len();
            s.launcher.move_caret(end, extend);
        }
        Key::Text(t) if !m.cmd && !m.ctrl => {
            let l = &mut s.launcher;
            l.replace_selection(t);
            l.actions = None;
            edited(s, tx);
        }
        _ => return false,
    }
    true
}

/// Presses keys by name, comma separated (`tab`, `enter`, `down`, `escape`, or any text), for
/// the snapshot tool to reach states a person gets to by typing.
pub fn press(s: &mut Stage, keys: &str) {
    for k in keys.split(',') {
        match k {
            "tab" => toggle_actions(s),
            "enter" => submit(s, Modifier::None),
            "escape" => escape(s),
            "down" | "up" => {
                let n = s.launcher.count();
                let l = &mut s.launcher;
                if let Some(a) = &mut l.actions {
                    a.sel = (a.sel + if k == "down" { 1 } else { a.items.len() - 1 }) % a.items.len().max(1);
                } else if n > 0 {
                    l.sel = (l.sel + if k == "down" { 1 } else { n - 1 }) % n;
                }
            }
            text => {
                s.launcher.replace_selection(text);
                requery(s, true);
            }
        }
    }
}
