//! Moves and clicks the pointer through wlr-virtual-pointer, for scripted bar tests.
//!
//! - `yz-pointer move X Y`: moves the cursor to layout coordinates X, Y.
//! - `yz-pointer click [hold-ms]`: clicks the left button where the cursor is (default 40 ms).
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
        _ => {
            eprintln!("usage: yz-pointer move X Y | yz-pointer click [hold-ms]");
            std::process::exit(2);
        }
    }
    pointer.destroy();
    sync(&mut queue);
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
