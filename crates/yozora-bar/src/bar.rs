//! One bar window: a layer-shell surface across the top of a monitor.

use std::{
    cell::RefCell,
    collections::{BTreeSet, HashMap},
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    AnyElement, App, Bounds, Context, Div, ElementId, FontWeight, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Pixels, Render, Styled, Subscription, WeakEntity, Window, WindowBackgroundAppearance, WindowBounds,
    WindowHandle, WindowKind, WindowOptions, canvas, div, layer_shell::*, point, prelude::*, px, size,
};
use serde_json::json;
use yozora_config::bar::BarStyle;
use yozora_ipc::{Incoming, Response};
use yozora_ui::{
    Theme,
    frame_stats::{FrameStats, Summary},
    spring::{Spring, SpringConfig},
};

use crate::{
    app::Config,
    modules::{self, Layout, Module, Pop},
    popover::Popover,
    services::{Kind, Services},
};

struct OpenPopover {
    pop: Pop,
    view: WeakEntity<Popover>,
    window: gpui::AnyWindowHandle,
    _released: Subscription,
}

struct Bench {
    what: String,
    remaining: u32,
    stats: FrameStats,
    incoming: Option<Incoming>,
    started: Instant,
}

pub struct Bar {
    pub monitor: String,
    pub layout: Layout,
    pub height: Pixels,
    handle: Option<WindowHandle<Bar>>,
    indicator: Spring,
    indicator_stats: FrameStats,
    indicator_last: Option<Summary>,
    popover: Option<OpenPopover>,
    /// Where each item was drawn, to anchor its popover.
    bounds: Rc<RefCell<HashMap<String, Bounds<Pixels>>>>,
    observed: BTreeSet<Kind>,
    /// Whether this bar already redraws on module host changes.
    modules_observed: bool,
    subscriptions: Vec<Subscription>,
    first_frame_logged: bool,
    bench: Option<Bench>,
    popover_stats: HashMap<&'static str, Summary>,
}

impl Bar {
    pub fn open(display: gpui::DisplayId, monitor: String, cx: &mut App) -> anyhow::Result<WindowHandle<Bar>> {
        let settings = Config::get(cx).settings.clone();
        let section = &settings.bar.bar;
        let height = px(section.height as f32);
        let floating = section.style == BarStyle::Floating;
        let (gap_top, gap_side) = if floating { (px(6.), px(8.)) } else { (px(0.), px(0.)) };
        let exclusive = if section.reserve_space { Some(height + gap_top) } else { None };
        let layout = Layout::parse(&settings.bar.zones_for(&monitor));
        let handle = cx.open_window(
            WindowOptions {
                titlebar: None,
                focus: false,
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: point(px(0.), px(0.)),
                    // Zero width stretches between the left and right anchors. GPUI's display
                    // bounds ignore output rotation, so they can't be trusted for this.
                    size: size(px(0.), height),
                })),
                app_id: Some("yozora-bar".into()),
                window_background: WindowBackgroundAppearance::Transparent,
                display_id: Some(display),
                // A bar never has keyboard focus. Without this GPUI caps it at 30 fps.
                inactive_frame_interval: None,
                kind: WindowKind::LayerShell(LayerShellOptions {
                    namespace: "yozora-bar".into(),
                    layer: Layer::Top,
                    anchor: Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
                    exclusive_zone: exclusive,
                    margin: floating.then_some((gap_top, gap_side, px(0.), gap_side)),
                    keyboard_interactivity: KeyboardInteractivity::None,
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                let handle = window.window_handle().downcast::<Bar>();
                cx.new(|cx| {
                    let mut bar = Self {
                        monitor,
                        layout,
                        height,
                        handle,
                        indicator: Spring::new(0.0, SpringConfig::INDICATOR),
                        indicator_stats: FrameStats::default(),
                        indicator_last: None,
                        popover: None,
                        bounds: Rc::default(),
                        observed: BTreeSet::new(),
                        modules_observed: false,
                        subscriptions: Vec::new(),
                        first_frame_logged: false,
                        bench: None,
                        popover_stats: HashMap::new(),
                    };
                    bar.observe_services(cx);
                    spawn_clock(cx);
                    bar
                })
            },
        )?;
        Ok(handle)
    }

    /// Redraws when a service this bar shows changes.
    pub fn observe_services(&mut self, cx: &mut Context<Self>) {
        let mut kinds = self.layout.services();
        kinds.insert(Kind::Hyprland);
        let mut subs = Vec::new();
        for kind in kinds {
            if self.observed.contains(&kind) {
                continue;
            }
            macro_rules! watch {
                ($field:ident) => {
                    Services::get(cx).$field.clone().map(|e| cx.observe(&e, |_, _, cx| cx.notify()))
                };
            }
            let sub = match kind {
                Kind::Audio => watch!(audio),
                Kind::Bluetooth => watch!(bluetooth),
                Kind::Custom => watch!(custom),
                Kind::Hyprland => watch!(hyprland),
                Kind::Media => watch!(media),
                Kind::Network => watch!(network),
                Kind::Notifications => watch!(notifications),
                Kind::Power => watch!(power),
                Kind::Tray => watch!(tray),
            };
            if let Some(sub) = sub {
                subs.push(sub);
                self.observed.insert(kind);
            }
        }
        if !self.modules_observed
            && let Some(host) = Services::get(cx).modules.clone()
        {
            subs.push(cx.observe(&host, |_, _, cx| cx.notify()));
            self.modules_observed = true;
        }
        self.subscriptions.extend(subs);
    }

    fn open_view(&self, cx: &App) -> Option<(Pop, gpui::Entity<Popover>)> {
        let p = self.popover.as_ref()?;
        let view = p.view.upgrade()?;
        (!view.read(cx).is_closing()).then(|| (p.pop.clone(), view))
    }

    pub fn popover_open(&self, pop: &Pop, cx: &App) -> bool {
        self.open_view(cx).is_some_and(|(p, _)| &p == pop)
    }

    fn module_active(&self, module: &Module, cx: &App) -> bool {
        self.open_view(cx).is_some_and(|(p, _)| &p.module() == module)
    }

    pub fn close_popover(&mut self, cx: &mut Context<Self>) {
        if let Some(view) = self.popover.as_ref().and_then(|p| p.view.upgrade()) {
            view.update(cx, |p, cx| p.close(cx));
        }
    }

    /// Opens `pop`, or closes it if it's open. `pressed` is true when called from a mouse press,
    /// which lets the popover take a grab so that clicking anywhere else dismisses it.
    pub fn toggle(&mut self, pop: Pop, pressed: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.popover_open(&pop, cx) {
            self.close_popover(cx);
            return;
        }
        // Replace whatever else is open right away.
        if let Some(p) = self.popover.take() {
            Popover::remove(p.window, cx);
        }
        let module = pop.module();
        let key = match &pop {
            Pop::Tray(k) => format!("tray/{k}"),
            _ => module.name(),
        };
        let item = self.bounds.borrow().get(&key).copied().unwrap_or(Bounds {
            origin: point(window.viewport_size().width - px(40.), px(0.)),
            size: size(px(30.), self.height),
        });
        // Span the bar's full height so the card sits 6 px under the bar, not under the item.
        let anchor = Bounds { origin: point(item.origin.x, px(0.)), size: size(item.size.width, self.height) };
        let left = matches!(pop, Pop::Menu) || self.layout.is_left(&module);
        let opened = Popover::open(
            window.window_handle(),
            anchor,
            pop.clone(),
            self.monitor.clone(),
            left,
            pressed,
            cx.weak_entity(),
            cx,
        );
        match opened {
            Ok(view) => {
                let id = view.entity_id();
                let released = cx.observe_release(&view, move |bar: &mut Self, _, cx| {
                    // A replaced popover may be released after its successor opened.
                    if bar.popover.as_ref().and_then(|p| p.view.upgrade()).is_none_or(|v| v.entity_id() == id) {
                        bar.popover = None;
                    }
                    cx.notify();
                });
                let window = view.read(cx).window();
                self.popover = Some(OpenPopover { pop, view: view.downgrade(), window, _released: released });
                cx.notify();
            }
            Err(e) => tracing::error!("could not open popover on {}: {e:#}", self.monitor),
        }
    }

    /// Called by a popover when its open or close animation settles, with its frame timing.
    pub fn popover_settled(
        &mut self,
        label: &'static str,
        stats: &mut FrameStats,
        refresh: Option<Duration>,
        cx: &mut Context<Self>,
    ) {
        let Some(bench) = self.bench.as_mut().filter(|b| b.what == "popover") else {
            if let Some(s) = stats.finish(label, refresh) {
                self.popover_stats.insert(label, s);
            }
            return;
        };
        bench.stats.absorb(stats);
        match label {
            "popover open" => self.close_popover(cx),
            _ => {
                bench.remaining = bench.remaining.saturating_sub(1);
                if bench.remaining == 0 {
                    self.finish_bench(refresh, cx);
                } else if let Some(handle) = self.handle {
                    // The closing window is still being torn down; open the next one after.
                    cx.defer(move |cx| {
                        let _ = handle.update(cx, |bar, window, cx| bar.toggle(Pop::ControlCenter, false, window, cx));
                    });
                }
            }
        }
    }

    fn finish_bench(&mut self, refresh: Option<Duration>, cx: &mut Context<Self>) {
        let Some(mut bench) = self.bench.take() else { return };
        let summary = bench.stats.summary(refresh);
        if let Some(s) = &summary {
            tracing::info!(target: "yozora::frames", "bench {} on {}: {s}", bench.what, self.monitor);
        }
        if let Some(incoming) = bench.incoming.take() {
            incoming.reply(Response::data(json!({
                "what": bench.what,
                "monitor": self.monitor,
                "seconds": bench.started.elapsed().as_secs_f64(),
                "frames": summary.as_ref().map(summary_json),
            })));
        }
        cx.notify();
    }

    /// Starts a benchmark that animates without touching real workspaces.
    pub fn bench(&mut self, what: &str, count: u32, incoming: Incoming, window: &mut Window, cx: &mut Context<Self>) {
        if self.bench.is_some() {
            incoming.reply(Response::error("a benchmark is already running"));
            return;
        }
        let bench = Bench {
            what: what.into(),
            remaining: count,
            stats: FrameStats::default(),
            incoming: Some(incoming),
            started: Instant::now(),
        };
        match what {
            "workspaces" => {
                // Each run is one sweep across the indicator and one back.
                self.bench = Some(Bench { remaining: count * 2, ..bench });
                let far = (modules::workspaces::slots(self, cx).len().max(2) - 1) as f32;
                self.indicator.set_target(if self.indicator.value() > 0.5 { 0.0 } else { far });
                self.indicator_stats = FrameStats::default();
                cx.notify();
            }
            "popover" => {
                if let Some(p) = self.popover.take() {
                    Popover::remove(p.window, cx);
                }
                self.bench = Some(bench);
                self.toggle(Pop::ControlCenter, false, window, cx);
            }
            other => {
                if let Some(i) = bench.incoming {
                    i.reply(Response::error(format!("unknown benchmark {other:?}; try workspaces or popover")));
                }
            }
        }
    }

    pub fn stats_json(&self) -> serde_json::Value {
        json!({
            "monitor": self.monitor,
            "workspace_indicator": self.indicator_last.as_ref().map(summary_json),
            "popover_open": self.popover_stats.get("popover open").map(summary_json),
            "popover_close": self.popover_stats.get("popover close").map(summary_json),
        })
    }

    pub fn benching_workspaces(&self) -> bool {
        self.bench.as_ref().is_some_and(|b| b.what == "workspaces")
    }

    /// Advances the workspace indicator for this frame and returns its position in slots.
    /// `target` is the active workspace's slot.
    pub fn indicator_position(
        &mut self,
        target: Option<usize>,
        reduced: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> f32 {
        let benching = self.benching_workspaces();
        if let (Some(t), false) = (target, benching)
            && self.indicator.target() != t as f32
        {
            self.indicator.set_target(t as f32);
        }
        if reduced && !benching {
            self.indicator = Spring::new(self.indicator.target(), SpringConfig::INDICATOR);
            return self.indicator.value();
        }
        let dt = self.indicator_stats.frame(Instant::now());
        if !self.indicator.step(dt) {
            window.request_animation_frame();
            return self.indicator.value();
        }
        let refresh = window.refresh_interval(cx);
        if let Some(bench) = self.bench.as_mut().filter(|b| b.what == "workspaces") {
            bench.stats.absorb(&mut self.indicator_stats);
            bench.remaining = bench.remaining.saturating_sub(1);
            if bench.remaining > 0 {
                let far = (modules::workspaces::slots(self, cx).len().max(2) - 1) as f32;
                let back = if self.indicator.target() > 0.0 { 0.0 } else { far };
                self.indicator.set_target(back);
                window.request_animation_frame();
            } else {
                self.finish_bench(refresh, cx);
                // Return to the real workspace.
                window.request_animation_frame();
            }
        } else {
            let label = format!("workspace indicator on {}", self.monitor);
            if let Some(s) = self.indicator_stats.finish(&label, refresh) {
                self.indicator_last = Some(s);
            }
        }
        self.indicator.value()
    }

    fn render_module(
        &mut self,
        module: &Module,
        theme: &Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let content: Div = match module {
            Module::Workspaces => return modules::workspaces::render(self, theme, window, cx),
            Module::Window => return modules::workspaces::window_title(self, theme, cx),
            Module::Tray => return modules::tray::render(self, theme, cx),
            Module::Custom(name) => return modules::custom::render(name, theme, cx),
            Module::Dynamic(name) => modules::module::item(name, theme, cx),
            Module::Launcher => modules::launcher::item(theme),
            Module::Clock => modules::calendar::item(theme, cx),
            Module::Media => modules::media::item(theme, cx)?,
            Module::Network => modules::network::item(theme, cx)?,
            Module::Sound => modules::sound::item(theme, cx)?,
            Module::Devices => modules::devices::item(theme, cx)?,
            Module::Battery => modules::devices::battery_item(theme, cx)?,
            Module::Notifications => modules::notifications::item(theme, cx)?,
            Module::ControlCenter => modules::control_center::item(theme),
        };
        let pop = module.popover()?;
        let active = self.module_active(module, cx);
        Some(self.button(module.name(), content, active, theme, cx, move |bar, window, cx| {
            bar.toggle(pop.clone(), true, window, cx)
        }))
    }

    /// Makes `content` a bar button: hover and active fills, bounds tracking, and acting on
    /// press rather than release, because a popup grab needs the press still active.
    pub fn button(
        &self,
        key: String,
        content: Div,
        active: bool,
        theme: &Theme,
        cx: &Context<Self>,
        on_press: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let fill_2 = theme.fill_2;
        content
            .id(ElementId::Name(key.clone().into()))
            .relative()
            .when(active, |el| el.bg(theme.fill_3))
            .when(!active, |el| el.hover(move |s| s.bg(fill_2)))
            .child(self.track(key))
            .on_mouse_down(MouseButton::Left, cx.listener(move |bar, _, window, cx| on_press(bar, window, cx)))
            .into_any_element()
    }

    /// Records where an element is drawn, for anchoring popovers to it.
    pub fn track(&self, key: String) -> impl IntoElement {
        let bounds = self.bounds.clone();
        canvas(
            move |b, _, _| {
                bounds.borrow_mut().insert(key.clone(), b);
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }
}

/// A bar item's base style, from the design's `.bi`.
pub fn bi() -> Div {
    div()
        .h(px(26.))
        .min_w(px(30.))
        .px(px(8.))
        .rounded(px(8.))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(7.))
        .flex_none()
}

pub fn summary_json(s: &Summary) -> serde_json::Value {
    json!({
        "frames": s.frames,
        "total_ms": s.total_ms,
        "mean_ms": s.mean_ms,
        "p50_ms": s.p50_ms,
        "p99_ms": s.p99_ms,
        "max_ms": s.max_ms,
        "refresh_ms": s.refresh_ms,
        "late": s.late,
    })
}

impl Render for Bar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.first_frame_logged {
            self.first_frame_logged = true;
            if let Some(started) = crate::STARTED.get() {
                tracing::info!("first frame on {} after {:?}", self.monitor, started.elapsed());
            }
        }
        let theme = Theme::global(cx).clone();
        let floating = crate::app::style(cx) == BarStyle::Floating;
        let mut zones: [Vec<AnyElement>; 3] = Default::default();
        let layout = self.layout.clone();
        for (i, modules) in [&layout.left, &layout.center, &layout.right].into_iter().enumerate() {
            for module in modules {
                if let Some(el) = self.render_module(module, &theme, window, cx) {
                    zones[i].push(el);
                }
            }
        }
        let [left, center, right] = zones;
        let zone = |items: Vec<AnyElement>| div().flex().items_center().gap(px(2.)).h_full().children(items);

        div()
            .size_full()
            .flex()
            .items_center()
            .px(px(6.))
            .bg(theme.glass_bar)
            .text_color(theme.ink)
            .text_size(px(13.))
            .font_family(theme.font.clone())
            .font_weight(FontWeight::MEDIUM)
            .when(floating, |el| el.rounded(px(12.)).border_1().border_color(theme.hair_2))
            .when(!floating, |el| el.border_b_1().border_color(theme.hair))
            .child(div().flex_1().flex().justify_start().min_w_0().overflow_hidden().child(zone(left)))
            .child(div().flex_none().flex().justify_center().child(zone(center)))
            .child(div().flex_1().flex().justify_end().min_w_0().child(zone(right)))
    }
}

/// Redraws at each minute boundary, or each second if the format shows seconds.
fn spawn_clock(cx: &mut Context<Bar>) {
    cx.spawn(async move |bar, cx| {
        loop {
            let seconds = cx.update(|cx| {
                let f = &Config::get(cx).settings.bar.clock.format;
                f.contains("%S") || f.contains("%T") || f.contains("%r") || f.contains("%X")
            });
            let now = chrono::Local::now();
            let nanos = u64::from(chrono::Timelike::nanosecond(&now) % 1_000_000_000);
            let into_second = Duration::from_nanos(nanos);
            let wait = if seconds {
                Duration::from_secs(1).saturating_sub(into_second)
            } else {
                let into_minute = Duration::from_secs(u64::from(chrono::Timelike::second(&now))) + into_second;
                Duration::from_secs(60).saturating_sub(into_minute)
            } + Duration::from_millis(5);
            cx.background_executor().timer(wait).await;
            if bar.update(cx, |_, cx| cx.notify()).is_err() {
                break;
            }
        }
    })
    .detach();
}
