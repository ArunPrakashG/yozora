//! Renders one popover on its own, for checking a screen without a compositor:
//!   YZ_POP=wifi YZ_LOOK=phosphor cargo run -p yozora-shell --bin snap -- --snap out.png --scale 2
//! `YZ_POP` is menu, battery, module (with `YZ_MODULE`), calendar, media, wifi, bluetooth, sound, control, tray, notifications or settings. Services run on fakes.

use rune::{AppSpec, Cx, El, col, el};
use yozora_services::Fakes;
use yozora_shell::shared::Shared;
use yozora_shell::stage::{Pop, Stage};
use yozora_style::FONTS;

fn init() -> Stage {
    let sh = Shared::new(yozora_shell::config::load(), Fakes(Fakes::ALL.iter().map(|s| s.to_string()).collect()));
    let mut st = Stage::new(sh, "snap", 1280.0);
    let pop = match std::env::var("YZ_POP").as_deref() {
        Ok("media") => Pop::Media,
        Ok("wifi") => Pop::Wifi,
        Ok("bluetooth") => Pop::Bluetooth,
        Ok("sound") => Pop::Sound,
        Ok("control") => Pop::Control,
        Ok("tray") => Pop::Tray,
        Ok("notifications") => Pop::Notifications,
        Ok("settings") => Pop::Settings,
        Ok("launcher") => Pop::Launcher,
        Ok("menu") => Pop::Menu,
        Ok("battery") => Pop::Battery,
        Ok("module") => Pop::Module,
        _ => Pop::Calendar,
    };
    // `YZ_POP=toasts` shows no panel, only the toasts the fake service lists in `YZ_TOASTS`.
    // `YZ_POP=tip` shows a bar tooltip under the right of the bar.
    if std::env::var("YZ_POP").as_deref() == Ok("tip") {
        yozora_shell::tooltip::set(&mut st, "snap", "Battery 64%\nOn battery \u{b7} 3 h 20 min left".into(), 1100.0);
    }
    if !matches!(std::env::var("YZ_POP").as_deref(), Ok("toasts") | Ok("tip")) {
        if pop == Pop::Module {
            // `YZ_MODULE=<name>` is a module installed in the config directory's `modules`.
            let name = std::env::var("YZ_MODULE").unwrap_or_default();
            st.sh.services.lock().unwrap().ensure_modules(&[name.clone()].into_iter().collect());
            std::thread::sleep(std::time::Duration::from_millis(800));
            st.open_module(&name, 1100.0);
        } else {
            st.toggle(pop, 1100.0);
        }
        if pop == Pop::Menu && std::env::var("YZ_CONFIRM").is_ok() {
            st.bar.confirm = Some(yozora_shell::menu::Session::Poweroff);
        }
    }
    notifications_state(&st);
    yozora_shell::notifications::preview_warm(&st);
    // `YZ_SUB=<id>[,<id>]` drills the tray menu into those submenus.
    if pop == Pop::Tray {
        // `YZ_ITEM` picks the fake tray item (`fake.chat` or `fake.sync`).
        let item = std::env::var("YZ_ITEM").unwrap_or_else(|_| "fake.chat".into());
        if let Some(tray) = &st.sh.services.lock().unwrap().tray {
            tray.send(yozora_services::tray::Cmd::LoadMenu(item));
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
        for id in std::env::var("YZ_SUB").unwrap_or_default().split(',').filter_map(|i| i.parse().ok()) {
            yozora_shell::tray::enter_submenu(id);
        }
    }
    if let Ok(p) = std::env::var("YZ_PAGE") {
        st.settings_page = p.parse().ok().or_else(|| yozora_shell::settings::page_named(&p)).unwrap_or(0);
    }
    st.anchor_x = 1100.0;
    if let Ok(r) = std::env::var("YZ_REPLY") {
        let (id, text) = r.split_once(':').unwrap_or((&r, ""));
        yozora_shell::notifications::preview_reply(&mut st, id.parse().unwrap_or(0), text);
    }
    if pop == Pop::Launcher {
        // The snapshot has no time to wait for the background reads, so do them here.
        let sh = st.sh.clone();
        let engine = sh.engine();
        engine.set_apps(yozora_search::index());
        engine.refresh_blocking();
        let query = std::env::var("YZ_QUERY").unwrap_or_default();
        yozora_shell::launcher::set_query(&mut st, &query);
        for _ in 0..40 {
            if engine.loading(&st.launcher.query()).is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        yozora_shell::launcher::sync(&mut st);
        if let Ok(keys) = std::env::var("YZ_KEYS") {
            yozora_shell::launcher::press(&mut st, &keys);
        }
    }
    st
}

/// `YZ_DND=1` turns Do Not Disturb on, `YZ_EMPTY=1` clears the center, `YZ_REPLY=<id>:<text>`
/// opens a reply field on that notification.
fn notifications_state(st: &Stage) {
    use yozora_services::notifications::Cmd;
    let send = |cmd: Cmd| {
        if let Some(n) = &st.sh.services.lock().unwrap().notifications {
            n.send(cmd);
        }
    };
    if std::env::var("YZ_DND").is_ok() {
        send(Cmd::SetDnd(true));
    }
    if std::env::var("YZ_EMPTY").is_ok() {
        send(Cmd::ClearAll);
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
}

fn view(s: &Stage, cx: &mut Cx<Stage>) -> El {
    let t = s.sh.tokens(s.sh.look().0);
    col().w_full().h_full().fill(t.bg).child(el().w_full().h_full().child(yozora_shell::stage::view(s, cx)))
}

fn main() {
    rune::run(AppSpec::<Stage> { fonts: FONTS, ..AppSpec::new("Yozora panel", (1280.0, if std::env::var("YZ_POP").as_deref() == Ok("settings") { 800.0 } else { 900.0 }), init, view) });
}
