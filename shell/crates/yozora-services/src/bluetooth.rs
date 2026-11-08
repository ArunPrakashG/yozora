//! BlueZ over the system bus, read through its ObjectManager.

use std::{collections::HashMap, time::Duration};

use zbus::{
    Connection,
    zvariant::{OwnedObjectPath, OwnedValue, Value},
};

use super::{
    Bluetooth, Link,
    dbus::{self, Wake},
};

const BLUEZ: &str = "org.bluez";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Device {
    pub path: String,
    pub address: String,
    pub name: String,
    /// BlueZ's icon name, like "audio-headphones" or "input-mouse".
    pub icon: String,
    pub connected: bool,
    pub paired: bool,
    pub battery: Option<u8>,
    pub busy: bool,
}

#[derive(Clone, Debug, Default)]
pub struct State {
    /// None when there is no adapter at all.
    pub adapter: Option<String>,
    pub powered: bool,
    /// The adapter is looking for devices nearby. Unpaired devices it finds are listed.
    pub scanning: bool,
    pub devices: Vec<Device>,
}

impl State {
    pub fn connected(&self) -> impl Iterator<Item = &Device> {
        self.devices.iter().filter(|d| d.connected)
    }
}

#[derive(Clone, Debug)]
pub enum Cmd {
    SetPowered(bool),
    Connect(String),
    Disconnect(String),
    /// Start or stop looking for devices nearby. A scan stops by itself after a while.
    Scan(bool),
}

/// How long a scan runs before it is stopped.
const SCAN_FOR: Duration = Duration::from_secs(25);

pub fn start(fake: bool) -> Bluetooth {
    if fake {
        super::spawn("bluetooth", true, run_fake)
    } else {
        super::spawn("bluetooth", false, |link| super::supervise(link, run))
    }
}

type Objects = HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>>;

async fn snapshot(conn: &Connection) -> anyhow::Result<State> {
    let objects: Objects =
        dbus::call(conn, BLUEZ, "/", "org.freedesktop.DBus.ObjectManager", "GetManagedObjects", &()).await?;
    let mut state = State::default();
    let mut adapters: Vec<_> = objects
        .iter()
        .filter_map(|(path, ifaces)| ifaces.get("org.bluez.Adapter1").map(|a| (path.to_string(), a)))
        .collect();
    adapters.sort_by(|a, b| a.0.cmp(&b.0));
    if let Some((path, adapter)) = adapters.first() {
        state.adapter = Some(path.clone());
        state.powered = dbus::get(adapter, "Powered").unwrap_or(false);
        state.scanning = dbus::get(adapter, "Discovering").unwrap_or(false);
    }
    for (path, ifaces) in &objects {
        let Some(d) = ifaces.get("org.bluez.Device1") else { continue };
        let paired: bool = dbus::get(d, "Paired").unwrap_or(false);
        let connected: bool = dbus::get(d, "Connected").unwrap_or(false);
        // Unpaired devices nearby are noise unless we can talk to them already, or the user
        // asked to look. Even then, a device that gives no name is just an address.
        if !paired && !connected && !(state.scanning && d.contains_key("Name")) {
            continue;
        }
        let name = dbus::get::<String>(d, "Alias").or_else(|| dbus::get(d, "Name")).unwrap_or_default();
        state.devices.push(Device {
            path: path.to_string(),
            address: dbus::string(d, "Address"),
            name,
            icon: dbus::string(d, "Icon"),
            connected,
            paired,
            battery: ifaces.get("org.bluez.Battery1").and_then(|b| dbus::get(b, "Percentage")),
            busy: false,
        });
    }
    state.devices.sort_by(|a, b| b.connected.cmp(&a.connected).then(b.paired.cmp(&a.paired)).then(a.name.cmp(&b.name)));
    Ok(state)
}

/// Waits until BlueZ appears on the bus. Calling it before then would try to start
/// bluetooth.service through D-Bus activation, which fails on machines without an adapter.
async fn wait_for_bluez(conn: &Connection, link: &Link<State, Cmd>) -> anyhow::Result<bool> {
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.freedesktop.DBus")?
        .member("NameOwnerChanged")?
        .arg(0, BLUEZ)?
        .build();
    let mut owners = dbus::signals(conn, rule).await?;
    while !dbus::name_has_owner(conn, BLUEZ).await {
        link.publish(State::default());
        match dbus::next(&link.cmds, &mut owners, Duration::from_millis(500)).await? {
            Wake::Quit => return Ok(false),
            _ => continue,
        }
    }
    Ok(true)
}

async fn run(link: &Link<State, Cmd>) -> anyhow::Result<()> {
    let conn = Connection::system().await?;
    if !wait_for_bluez(&conn, link).await? {
        return Ok(());
    }
    let mut stream = dbus::signals_from(&conn, BLUEZ).await?;
    let mut state = snapshot(&conn).await?;
    link.publish(state.clone());
    let mut scan_until: Option<std::time::Instant> = None;
    loop {
        if scan_until.is_some_and(|t| t <= std::time::Instant::now()) {
            scan_until = None;
            if let Some(a) = &state.adapter {
                let _ = dbus::invoke(&conn, BLUEZ, a, "org.bluez.Adapter1", "StopDiscovery", &()).await;
            }
        }
        match dbus::next(&link.cmds, &mut stream, Duration::from_millis(80)).await? {
            Wake::Quit => return Ok(()),
            Wake::Changed => {}
            Wake::Command(cmd) => {
                let result = match &cmd {
                    Cmd::SetPowered(on) => match &state.adapter {
                        Some(a) => {
                            dbus::set_prop(&conn, BLUEZ, a, "org.bluez.Adapter1", "Powered", Value::from(*on)).await
                        }
                        None => Err(anyhow::anyhow!("no Bluetooth adapter")),
                    },
                    Cmd::Scan(on) => match &state.adapter {
                        Some(a) => {
                            scan_until = on.then(|| std::time::Instant::now() + SCAN_FOR);
                            let method = if *on { "StartDiscovery" } else { "StopDiscovery" };
                            dbus::invoke(&conn, BLUEZ, a, "org.bluez.Adapter1", method, &()).await
                        }
                        None => Err(anyhow::anyhow!("no Bluetooth adapter")),
                    },
                    Cmd::Connect(path) | Cmd::Disconnect(path) => {
                        // Connecting takes seconds; show it as busy meanwhile.
                        let unpaired = state.devices.iter().any(|d| &d.path == path && !d.paired);
                        if let Some(d) = state.devices.iter_mut().find(|d| &d.path == path) {
                            d.busy = true;
                        }
                        link.publish(state.clone());
                        let dev = "org.bluez.Device1";
                        if unpaired && matches!(cmd, Cmd::Connect(_)) {
                            // A device found by scanning is paired and trusted first, so it
                            // reconnects by itself next time.
                            match dbus::invoke(&conn, BLUEZ, path, dev, "Pair", &()).await {
                                Ok(()) => {
                                    let _ = dbus::set_prop(&conn, BLUEZ, path, dev, "Trusted", Value::from(true)).await;
                                }
                                Err(e) => tracing::warn!("bluetooth: pairing: {e:#}"),
                            }
                        }
                        let method = if matches!(cmd, Cmd::Connect(_)) { "Connect" } else { "Disconnect" };
                        dbus::invoke(&conn, BLUEZ, path, dev, method, &()).await
                    }
                };
                if let Err(e) = result {
                    tracing::warn!("bluetooth: {e:#}");
                }
            }
        }
        state = snapshot(&conn).await?;
        link.publish(state.clone());
    }
}

fn run_fake(link: Link<State, Cmd>) {
    let dev = |n: &str, icon: &str, connected, battery| Device {
        path: format!("/fake/{n}"),
        address: format!("fake:{n}"),
        name: n.into(),
        icon: icon.into(),
        connected,
        paired: true,
        battery,
        busy: false,
    };
    let mut state = State {
        adapter: Some("/fake/hci0".into()),
        powered: true,
        scanning: false,
        devices: vec![
            dev("Headphones", "audio-headphones", true, Some(80)),
            dev("Keyboard", "input-keyboard", true, Some(72)),
            dev("Mouse", "input-mouse", true, Some(45)),
            dev("Desk Speaker", "audio-speakers", false, None),
        ],
    };
    link.publish(state.clone());
    while let Ok(cmd) = link.cmds.recv_blocking() {
        match cmd {
            Cmd::SetPowered(on) => {
                state.powered = on;
                for d in &mut state.devices {
                    d.connected = on && d.battery.is_some();
                }
            }
            Cmd::Scan(on) => {
                state.scanning = on && state.powered;
                state.devices.retain(|d| d.paired);
                if state.scanning {
                    let mut found = dev("Soundbar", "audio-speakers", false, None);
                    found.paired = false;
                    state.devices.push(found);
                }
            }
            Cmd::Connect(path) | Cmd::Disconnect(path) => {
                let i = state.devices.iter().position(|d| d.path == path);
                if let Some(i) = i {
                    state.devices[i].busy = true;
                    link.publish(state.clone());
                    std::thread::sleep(Duration::from_millis(700));
                    let d = &mut state.devices[i];
                    d.busy = false;
                    d.paired = true;
                    d.connected = !d.connected;
                }
            }
        }
        if !link.publish(state.clone()) {
            break;
        }
    }
}

pub async fn check() -> anyhow::Result<String> {
    let conn = Connection::system().await?;
    if !dbus::name_has_owner(&conn, BLUEZ).await {
        return Ok("BlueZ is not running (no adapter, or bluetooth.service is stopped)".into());
    }
    let s = snapshot(&conn).await?;
    Ok(match s.adapter {
        Some(a) => {
            format!("BlueZ · {a} {} · {} paired device(s)", if s.powered { "on" } else { "off" }, s.devices.len())
        }
        None => "BlueZ · no adapter".into(),
    })
}
