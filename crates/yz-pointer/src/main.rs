//! Moves and clicks the pointer through wlr-virtual-pointer, for scripted bar tests.
//!
//! - `yz-pointer move X Y`: moves the cursor to layout coordinates X, Y.
//! - `yz-pointer click [hold-ms]`: clicks the left button where the cursor is (default 40 ms).
//! - `yz-pointer drag X Y [steps]`: presses where the cursor is, moves to X, Y in steps, and
//!   releases there.
//!
//! Scripts should move, check `hyprctl cursorpos`, and only then click, so a click never lands
//! somewhere unexpected.

use std::{thread, time::Duration};

use wayland_client::{
    Connection, Dispatch, EventQueue, QueueHandle, delegate_noop,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{wl_pointer::ButtonState, wl_registry},
};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};

const BTN_LEFT: u32 = 0x110;
/// The bounding box of the monitor layout. Hyprland maps absolute motion over all outputs.
const LAYOUT_ORIGIN: (i32, i32) = (-1080, 0);
const LAYOUT_EXTENT: (u32, u32) = (3000, 2560);

struct State;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
delegate_noop!(State: ignore ZwlrVirtualPointerManagerV1);
delegate_noop!(State: ignore ZwlrVirtualPointerV1);

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let conn = Connection::connect_to_env().expect("no Wayland display");
    let (globals, mut queue) = registry_queue_init::<State>(&conn).expect("registry");
    let qh = queue.handle();
    let manager: ZwlrVirtualPointerManagerV1 = globals
        .bind(&qh, 1..=2, ())
        .expect("compositor lacks zwlr_virtual_pointer_manager_v1");
    let pointer = manager.create_virtual_pointer(None, &qh, ());

    match args.first().map(String::as_str) {
        Some("move") => {
            let coord = |i: usize| -> i32 {
                args.get(i)
                    .and_then(|a| a.parse().ok())
                    .expect("usage: yz-pointer move X Y")
            };
            let (x, y) = (coord(1) - LAYOUT_ORIGIN.0, coord(2) - LAYOUT_ORIGIN.1);
            pointer.motion_absolute(time(), x as u32, y as u32, LAYOUT_EXTENT.0, LAYOUT_EXTENT.1);
            pointer.frame();
            sync(&mut queue);
        }
        Some("click") => {
            let hold = args
                .get(1)
                .and_then(|a| a.parse().ok())
                .map_or(Duration::from_millis(40), Duration::from_millis);
            pointer.button(time(), BTN_LEFT, ButtonState::Pressed);
            pointer.frame();
            sync(&mut queue);
            thread::sleep(hold);
            pointer.button(time(), BTN_LEFT, ButtonState::Released);
            pointer.frame();
            sync(&mut queue);
        }
        Some("drag") => {
            let arg = |i: usize| -> Option<i32> { args.get(i).and_then(|a| a.parse().ok()) };
            let (Some(x), Some(y)) = (arg(1), arg(2)) else {
                eprintln!("usage: yz-pointer drag X Y [steps]");
                std::process::exit(2);
            };
            let steps = arg(3).unwrap_or(20).max(1);
            let (from_x, from_y) = cursor().expect("hyprctl cursorpos");
            pointer.button(time(), BTN_LEFT, ButtonState::Pressed);
            pointer.frame();
            sync(&mut queue);
            for i in 1..=steps {
                let t = i as f32 / steps as f32;
                let px = from_x + ((x - from_x) as f32 * t).round() as i32 - LAYOUT_ORIGIN.0;
                let py = from_y + ((y - from_y) as f32 * t).round() as i32 - LAYOUT_ORIGIN.1;
                pointer.motion_absolute(time(), px as u32, py as u32, LAYOUT_EXTENT.0, LAYOUT_EXTENT.1);
                pointer.frame();
                sync(&mut queue);
                thread::sleep(Duration::from_millis(16));
            }
            thread::sleep(Duration::from_millis(80));
            pointer.button(time(), BTN_LEFT, ButtonState::Released);
            pointer.frame();
            sync(&mut queue);
        }
        _ => {
            eprintln!("usage: yz-pointer move X Y | yz-pointer click [hold-ms] | yz-pointer drag X Y [steps]");
            std::process::exit(2);
        }
    }
    pointer.destroy();
    sync(&mut queue);
}

/// The cursor position, from Hyprland.
fn cursor() -> Option<(i32, i32)> {
    let out = std::process::Command::new("hyprctl").arg("cursorpos").output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    let (x, y) = text.trim().split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

fn sync(queue: &mut EventQueue<State>) {
    queue.roundtrip(&mut State).expect("Wayland roundtrip");
}

/// Milliseconds since an arbitrary epoch, as the protocol wants.
fn time() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u32
}
