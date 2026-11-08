//! Renders one popover on its own, for checking a screen without a compositor:
//!   YZ_POP=wifi YZ_LOOK=phosphor cargo run -p yozora-shell --bin snap -- --snap out.png --scale 2
//! `YZ_POP` is calendar, media, wifi, bluetooth, sound, control or tray. Services run on fakes.

use rune::{AppSpec, Cx, El, col, el};
use yozora_services::Fakes;
use yozora_shell::shared::Shared;
use yozora_shell::stage::{Pop, Stage};
use yozora_style::{FONTS, Fx, Look, tokens};

fn init() -> Stage {
    let look = std::env::var("YZ_LOOK").ok().and_then(|s| Look::from_slug(&s)).unwrap_or_default();
    let fx = std::env::var("YZ_FX").ok().and_then(|s| Fx::from_slug(&s)).unwrap_or(Fx::Full);
    let sh = Shared::new(look, fx, Fakes(Fakes::ALL.iter().map(|s| s.to_string()).collect()));
    let mut st = Stage::new(sh, "snap", 1280.0);
    let pop = match std::env::var("YZ_POP").as_deref() {
        Ok("media") => Pop::Media,
        Ok("wifi") => Pop::Wifi,
        Ok("bluetooth") => Pop::Bluetooth,
        Ok("sound") => Pop::Sound,
        Ok("control") => Pop::Control,
        Ok("tray") => Pop::Tray,
        Ok("notifications") => Pop::Notifications,
        _ => Pop::Calendar,
    };
    st.toggle(pop, 1100.0);
    st.anchor_x = 1100.0;
    st
}

fn view(s: &Stage, cx: &mut Cx<Stage>) -> El {
    let t = tokens(s.sh.look().0);
    col().w_full().h_full().fill(t.bg).child(el().w_full().h_full().child(yozora_shell::stage::view(s, cx)))
}

fn main() {
    rune::run(AppSpec::<Stage> { fonts: FONTS, ..AppSpec::new("Yozora panel", (1280.0, 900.0), init, view) });
}
