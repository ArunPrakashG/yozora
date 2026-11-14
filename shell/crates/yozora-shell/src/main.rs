
use std::time::Duration;

use chrono::Timelike;

use rune::{
    Align, App, AppSpec, Cx, DisplayTarget, El, KeyPolicy, OverlayInput, OverlayOptions, OverlayPlacement, OverlaySpec, RunOptions, ScreenEdge, WaylandLayer,
    WindowSpec, col,
};
use yozora_services::Fakes;
use yozora_style::FONTS;

use yozora_shell::bar::{self, Bar};
use yozora_shell::shared::Shared;
use yozora_shell::stage::{self, Stage};

fn host(_: &(), _: &mut Cx<()>) -> El {
    col()
}

fn main() -> std::process::ExitCode {
    // `yozora-shell msg ...` and `osd ...` talk to the running shell instead of starting one.
    if let Some(code) = yozora_shell::ipc::cli(&std::env::args().skip(1).collect::<Vec<_>>()) {
        return code;
    }
    let config = yozora_shell::config::load();
    let only = std::env::var("YZ_OUT").ok();
    let fakes = Fakes(std::env::var("YZ_FAKE").ok().map(|s| s.split(',').map(str::to_owned).collect()).unwrap_or_default());
    rune::run_windows_with(RunOptions { accessory: true }, move |displays| {
        let sh = Shared::new(config, fakes);
        let mut app = App::new(AppSpec { fonts: FONTS, ..AppSpec::new("Yozora shell", (10.0, 10.0), || (), host) }).windowless();
        // The geometry the bar will have, before anything draws.
        sh.set_bar_conf(std::sync::Arc::new(yozora_shell::bar_config::load()));
        let height = Bar::height(&sh);
        let exclusive = sh.bar_conf().exclusive_zone(&sh.bar_conf().geo(&yozora_style::tokens(sh.look().0)));
        for d in displays.iter().filter(|d| only.is_none() || d.name == only) {
            let name = d.name.clone().unwrap_or_default();
            let target = d.id.clone().map_or(DisplayTarget::Primary, DisplayTarget::Id);
            let w = d.size.0 as f32 / d.scale as f32;
            let bar = app.overlay(
                OverlaySpec { edge: ScreenEdge::Top, align: Align::Center, ..OverlaySpec::new("yozora-bar", (w, height), bar::view) }.options(OverlayOptions {
                    wayland_layer: WaylandLayer::Top,
                    exclusive_zone: exclusive,
                    input: OverlayInput::Regions,
                    display: target.clone(),
                    ..Default::default()
                }),
                Bar { sh: sh.clone(), output: name.clone(), screen_w: w, anchors: Default::default(), wheel: 0.0 },
            );
            let stage = app.overlay(
                OverlaySpec::new("yozora-stage", (w, 10.0), stage::view)
                    .options(OverlayOptions {
                        placement: Some(OverlayPlacement::FullScreen),
                        // The whole output, under the bar's zone too: panels, tooltips and the drop
                        // are placed in screen points from its top.
                        exclusive_zone: -1,
                        wayland_layer: WaylandLayer::Top,
                        input: OverlayInput::Regions,
                        keys: KeyPolicy::OnDemand,
                        display: target,
                        ..Default::default()
                    })
                    .on_key(stage::on_key)
                    .animate_reveal(),
                Stage::new(sh.clone(), &name, w),
            );
            // `bar_config::apply` shows each bar once it knows whether `bar.toml` wants it here.
            stage.show();
            sh.attach_bar_on(&name, w, bar);
            sh.attach_stage(&name, stage);
        }
        // Redraw when a service changes, and every few seconds so the clock keeps time.
        let wake = sh.clone();
        yozora_services::set_waker(move || wake.redraw());
        yozora_shell::config::watch(sh.clone());
        yozora_shell::bar_config::apply(&sh);
        yozora_shell::bar_config::watch(sh.clone());
        yozora_shell::ipc::start(&sh);
        let tick = sh.clone();
        // The clock redraws on the minute, or every second when its format shows seconds.
        std::thread::Builder::new().name("clock".into()).spawn(move || loop {
            let seconds = bar::clock_ticks_seconds(&tick.bar_conf().bar.clock.format);
            let now = chrono::Local::now();
            let next = if seconds {
                1_000 - u64::from(now.timestamp_subsec_millis())
            } else {
                (60 - u64::from(now.second())) * 1_000 - u64::from(now.timestamp_subsec_millis())
            };
            std::thread::sleep(Duration::from_millis(next.max(20) + 5));
            tick.redraw();
        }).ok();
        vec![WindowSpec::from_app("host", app)]
    });
    std::process::ExitCode::SUCCESS
}
