//! Batteries from UPower, and power profiles from power-profiles-daemon (under either of its
//! bus names).

use std::time::Duration;

use futures_lite::FutureExt;
use gpui::{App, Entity};
use zbus::{
    Connection,
    zvariant::{OwnedObjectPath, OwnedValue, Value},
};

use super::{
    Link, Power,
    dbus::{self, Wake},
};

const UPOWER: &str = "org.freedesktop.UPower";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Charge {
    Charging,
    #[default]
    Discharging,
    Full,
    NotCharging,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Battery {
    pub percent: f32,
    pub charge: Charge,
    /// Seconds until empty or full, when UPower knows.
    pub seconds_left: Option<i64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeviceKind {
    Mouse,
    Keyboard,
    Headphones,
    Gamepad,
    Phone,
    #[default]
    Other,
}

/// A peripheral that reports its own battery, like a wireless mouse.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Peripheral {
    pub name: String,
    pub kind: DeviceKind,
    pub percent: f32,
    /// Bluetooth address, when UPower got it from BlueZ, to match up with Bluetooth devices.
    pub address: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub battery: Option<Battery>,
    pub peripherals: Vec<Peripheral>,
    /// Profile names in order, like ["power-saver", "balanced", "performance"].
    pub profiles: Vec<String>,
    pub profile: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Cmd {
    SetProfile(String),
}

pub fn start(cx: &mut App, fake: bool) -> Entity<Power> {
    if fake {
        super::spawn(cx, "power", true, run_fake)
    } else {
        super::spawn(cx, "power", false, |link| super::supervise(link, run))
    }
}

pub fn profile_label(name: &str) -> &str {
    match name {
        "power-saver" => "Saver",
        "balanced" => "Balanced",
        "performance" => "Performance",
        other => other,
    }
}

/// power-profiles-daemon 0.20 moved under UPower's name; older releases use net.hadess.
const PROFILES: &[(&str, &str, &str)] = &[
    (
        "org.freedesktop.UPower.PowerProfiles",
        "/org/freedesktop/UPower/PowerProfiles",
        "org.freedesktop.UPower.PowerProfiles",
    ),
    ("net.hadess.PowerProfiles", "/net/hadess/PowerProfiles", "net.hadess.PowerProfiles"),
];

async fn profiles_endpoint(conn: &Connection) -> Option<(&'static str, &'static str, &'static str)> {
    for p in PROFILES {
        if dbus::name_has_owner(conn, p.0).await {
            return Some(*p);
        }
    }
    None
}

fn kind(upower_type: u32) -> DeviceKind {
    match upower_type {
        5 => DeviceKind::Mouse,
        6 | 14 => DeviceKind::Keyboard,
        17 | 18 | 19 | 21 => DeviceKind::Headphones,
        12 => DeviceKind::Gamepad,
        8 => DeviceKind::Phone,
        _ => DeviceKind::Other,
    }
}

async fn snapshot(conn: &Connection, profiles: Option<(&str, &str, &str)>) -> anyhow::Result<State> {
    let mut state = State::default();
    let paths: Vec<OwnedObjectPath> =
        dbus::call(conn, UPOWER, "/org/freedesktop/UPower", UPOWER, "EnumerateDevices", &()).await?;
    for path in paths {
        let Ok(d) = dbus::get_all(conn, UPOWER, &path, "org.freedesktop.UPower.Device").await else { continue };
        let ty: u32 = dbus::get(&d, "Type").unwrap_or(0);
        let percent: f64 = dbus::get(&d, "Percentage").unwrap_or(0.0);
        let present: bool = dbus::get(&d, "IsPresent").unwrap_or(true);
        if ty == 1 || !present {
            continue;
        }
        if ty == 2 && dbus::get::<bool>(&d, "PowerSupply").unwrap_or(false) {
            // Several laptop batteries are rare; UPower's DisplayDevice would merge them, but the
            // first is what nearly everyone has.
            if state.battery.is_some() {
                continue;
            }
            let charge = match dbus::get::<u32>(&d, "State").unwrap_or(0) {
                1 | 5 => Charge::Charging,
                4 => Charge::Full,
                6 => Charge::NotCharging,
                _ => Charge::Discharging,
            };
            let secs = match charge {
                Charge::Charging => dbus::get::<i64>(&d, "TimeToFull"),
                Charge::Discharging => dbus::get::<i64>(&d, "TimeToEmpty"),
                _ => None,
            }
            .filter(|s| *s > 0);
            state.battery = Some(Battery { percent: percent as f32, charge, seconds_left: secs });
        } else {
            let native = dbus::string(&d, "NativePath");
            let model = dbus::string(&d, "Model");
            // BlueZ-backed devices have paths like "/org/bluez/hci0/dev_AA_BB_…".
            let address = native.rsplit_once("dev_").map(|(_, a)| a.split('/').next().unwrap_or(a).replace('_', ":"));
            state.peripherals.push(Peripheral {
                name: if model.is_empty() { "Device".into() } else { model },
                kind: kind(ty),
                percent: percent as f32,
                address,
            });
        }
    }
    if let Some((dest, path, iface)) = profiles
        && let Ok(p) = dbus::get_all(conn, dest, path, iface).await
    {
        state.profile = dbus::get::<String>(&p, "ActiveProfile");
        let list: Vec<std::collections::HashMap<String, OwnedValue>> = dbus::get(&p, "Profiles").unwrap_or_default();
        let order = ["power-saver", "balanced", "performance"];
        let mut names: Vec<String> = list.iter().map(|m| dbus::string(m, "Profile")).collect();
        names.sort_by_key(|n| order.iter().position(|o| o == n).unwrap_or(order.len()));
        state.profiles = names;
    }
    Ok(state)
}

async fn run(link: &Link<State, Cmd>) -> anyhow::Result<()> {
    let conn = Connection::system().await?;
    let profiles = profiles_endpoint(&conn).await;
    let mut upower = dbus::signals_from(&conn, UPOWER).await?;
    let mut ppd = match profiles {
        Some(p) => Some(dbus::signals_from(&conn, p.0).await?),
        None => None,
    };
    link.publish(snapshot(&conn, profiles).await?);
    loop {
        let wake = async { dbus::next(&link.cmds, &mut upower, Duration::from_millis(200)).await }
            .or(async {
                match ppd.as_mut() {
                    Some(s) => dbus::next(&link.cmds, s, Duration::from_millis(50)).await,
                    None => std::future::pending().await,
                }
            })
            .await?;
        match wake {
            Wake::Quit => return Ok(()),
            Wake::Changed => {}
            Wake::Command(Cmd::SetProfile(name)) => {
                if let Some((dest, path, iface)) = profiles
                    && let Err(e) = dbus::set_prop(&conn, dest, path, iface, "ActiveProfile", Value::from(name)).await
                {
                    tracing::warn!("power profile: {e:#}");
                }
            }
        }
        link.publish(snapshot(&conn, profiles).await?);
    }
}

fn run_fake(link: Link<State, Cmd>) {
    let mut state = State {
        battery: Some(Battery { percent: 64.0, charge: Charge::Discharging, seconds_left: Some(3 * 3600 + 20 * 60) }),
        peripherals: vec![
            Peripheral {
                name: "Headphones".into(),
                kind: DeviceKind::Headphones,
                percent: 80.0,
                address: Some("fake:1".into()),
            },
            Peripheral {
                name: "Keyboard".into(),
                kind: DeviceKind::Keyboard,
                percent: 72.0,
                address: Some("fake:2".into()),
            },
            Peripheral { name: "Mouse".into(), kind: DeviceKind::Mouse, percent: 45.0, address: Some("fake:3".into()) },
        ],
        profiles: vec!["power-saver".into(), "balanced".into(), "performance".into()],
        profile: Some("balanced".into()),
    };
    link.publish(state.clone());
    while let Ok(Cmd::SetProfile(name)) = link.cmds.recv_blocking() {
        state.profile = Some(name);
        if !link.publish(state.clone()) {
            break;
        }
    }
}

pub async fn check() -> anyhow::Result<String> {
    let conn = Connection::system().await?;
    let profiles = profiles_endpoint(&conn).await;
    let s = snapshot(&conn, profiles).await?;
    let battery = s.battery.map(|b| format!("battery {:.0}%", b.percent)).unwrap_or("no battery".into());
    let profile = s.profile.map(|p| format!("profile {p}")).unwrap_or("no power-profiles-daemon".into());
    Ok(format!("UPower · {battery} · {} peripheral(s) · {profile}", s.peripherals.len()))
}
