//! Idle detection with ext-idle-notify-v1: the compositor says when the seat has had no input
//! for a while, and counts anything that asks for the screen to stay awake.

use async_channel::Sender;
use wayland_client::{
    Connection, Dispatch, QueueHandle,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{wl_registry, wl_seat},
};
use wayland_protocols::ext::idle_notify::v1::client::{
    ext_idle_notification_v1::{self, ExtIdleNotificationV1},
    ext_idle_notifier_v1::ExtIdleNotifierV1,
};

use crate::{Msg, Stage};

struct State {
    tx: Sender<Msg>,
}

/// Watches for idleness until the connection ends. `timeouts` is each stage with its wait in
/// milliseconds.
pub fn run(timeouts: Vec<(Stage, u32)>, tx: Sender<Msg>) -> anyhow::Result<()> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<State>(&conn)?;
    let qh = queue.handle();
    let seat: wl_seat::WlSeat = globals.bind(&qh, 1..=1, ())?;
    let notifier: ExtIdleNotifierV1 = globals
        .bind(&qh, 1..=1, ())
        .map_err(|_| anyhow::anyhow!("the compositor doesn't support ext-idle-notify-v1"))?;
    let _notifications: Vec<ExtIdleNotificationV1> =
        timeouts.iter().map(|(stage, ms)| notifier.get_idle_notification(*ms, &seat, &qh, *stage)).collect();
    let mut state = State { tx };
    loop {
        queue.blocking_dispatch(&mut state)?;
    }
}

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

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(_: &mut Self, _: &wl_seat::WlSeat, _: wl_seat::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ExtIdleNotifierV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ExtIdleNotifierV1,
        _: <ExtIdleNotifierV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtIdleNotificationV1, Stage> for State {
    fn event(
        state: &mut Self,
        _: &ExtIdleNotificationV1,
        event: ext_idle_notification_v1::Event,
        stage: &Stage,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let msg = match event {
            ext_idle_notification_v1::Event::Idled => Msg::Idled(*stage),
            ext_idle_notification_v1::Event::Resumed => Msg::Resumed(*stage),
            _ => return,
        };
        let _ = state.tx.send_blocking(msg);
    }
}
