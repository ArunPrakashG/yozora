mod bar;
mod bluetooth;
mod calendar;
mod control;
mod kit;
mod media;
mod shared;
mod sound;
mod stage;
mod tray;
mod wifi;

use std::time::Duration;

use rune::{
    Align, App, AppSpec, Cx, DisplayTarget, El, KeyPolicy, OverlayInput, OverlayOptions, OverlayPlacement, OverlaySpec, RunOptions, ScreenEdge, WaylandLayer,
    WindowSpec, col,
};
use yozora_services::Fakes;
use yozora_style::{FONTS, Fx, Look};

use bar::Bar;
use shared::Shared;
use stage::Stage;

fn host(_: &(), _: &mut Cx<()>) -> El {
    col()
}

fn main() {
    let look = std::env::var("YZ_LOOK").ok().and_then(|s| Look::from_slug(&s)).unwrap_or_default();
    let only = std::env::var("YZ_OUT").ok();
    let fakes = Fakes(std::env::var("YZ_FAKE").ok().map(|s| s.split(',').map(str::to_owned).collect()).unwrap_or_default());
    rune::run_windows_with(RunOptions { accessory: true }, move |displays| {
        let sh = Shared::new(look, Fx::Full, fakes);
        let mut app = App::new(AppSpec { fonts: FONTS, ..AppSpec::new("Yozora shell", (10.0, 10.0), || (), host) }).windowless();
        let height = Bar::height(&sh);
        for d in displays.iter().filter(|d| only.is_none() || d.name == only) {
            let name = d.name.clone().unwrap_or_default();
            let target = d.id.clone().map_or(DisplayTarget::Primary, DisplayTarget::Id);
            let w = d.size.0 as f32 / d.scale as f32;
            let bar = app.overlay(
                OverlaySpec { edge: ScreenEdge::Top, align: Align::Center, ..OverlaySpec::new("yozora-bar", (w, height), bar::view) }.options(OverlayOptions {
                    wayland_layer: WaylandLayer::Top,
                    exclusive_zone: height as i32,
                    input: OverlayInput::Regions,
                    display: target.clone(),
                    ..Default::default()
                }),
                Bar { sh: sh.clone(), output: name.clone(), screen_w: w, anchors: Default::default() },
            );
            let stage = app.overlay(
                OverlaySpec::new("yozora-stage", (w, 10.0), stage::view)
                    .options(OverlayOptions {
                        placement: Some(OverlayPlacement::FullScreen),
                        wayland_layer: WaylandLayer::Top,
                        input: OverlayInput::Regions,
                        keys: KeyPolicy::OnDemand,
                        display: target,
                        ..Default::default()
                    })
                    .on_key(stage::on_key)
                    .animate_reveal(),
                Stage::new(sh.clone(), w),
            );
            bar.show();
            stage.show();
            sh.attach_bar(bar);
            sh.attach_stage(&name, stage);
        }
        // Redraw when a service changes, and every few seconds so the clock keeps time.
        let wake = sh.clone();
        yozora_services::set_waker(move || wake.redraw());
        let tick = sh.clone();
        std::thread::Builder::new().name("clock".into()).spawn(move || loop {
            std::thread::sleep(Duration::from_secs(15));
            tick.redraw();
        }).ok();
        vec![WindowSpec::from_app("host", app)]
    });
}
