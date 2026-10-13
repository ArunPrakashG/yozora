//! NetworkManager over the system bus.

use std::{collections::HashMap, time::Duration};

use gpui::{App, Entity};
use zbus::{
    Connection,
    zvariant::{OwnedObjectPath, OwnedValue, Value},
};

use super::{
    Link, Network,
    dbus::{self, Props, Wake},
};

const NM: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    Wifi,
    Ethernet,
    Vpn,
    #[default]
    Other,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Primary {
    pub name: String,
    pub kind: Kind,
    /// Signal strength 0..=100 for Wi-Fi.
    pub strength: u8,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AccessPoint {
    pub ssid: String,
    pub strength: u8,
    pub secured: bool,
    /// "WPA2", "WPA3" or empty.
    pub security: String,
    pub frequency: u32,
    pub active: bool,
    /// A saved connection exists for this SSID.
    pub known: bool,
    path: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Vpn {
    pub name: String,
    pub uuid: String,
    pub active: bool,
    pub connecting: bool,
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub primary: Option<Primary>,
    pub wifi_enabled: bool,
    /// A Wi-Fi device exists.
    pub wifi_device: bool,
    pub wired: Option<Wired>,
    pub access_points: Vec<AccessPoint>,
    pub vpns: Vec<Vpn>,
    /// Connecting to this SSID right now.
    pub activating: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Wired {
    pub interface: String,
    pub connected: bool,
}

impl State {
    pub fn connected(&self) -> bool {
        self.primary.is_some()
    }
}

#[derive(Clone, Debug)]
pub enum Cmd {
    SetWifi(bool),
    Connect(String),
    Disconnect,
    SetVpn(String, bool),
    Scan,
}

pub fn start(cx: &mut App, fake: bool) -> Entity<Network> {
    if fake {
        super::spawn(cx, "network", true, run_fake)
    } else {
        super::spawn(cx, "network", false, |link| super::supervise(link, run))
    }
}

struct Device {
    path: String,
    kind: u32,
    interface: String,
    state: u32,
}

async fn devices(conn: &Connection) -> anyhow::Result<Vec<Device>> {
    let paths: Vec<OwnedObjectPath> = dbus::call(conn, NM, NM_PATH, NM, "GetDevices", &()).await?;
    let mut out = Vec::new();
    for path in paths {
        let props = dbus::get_all(conn, NM, &path, "org.freedesktop.NetworkManager.Device").await?;
        out.push(Device {
            path: path.to_string(),
            kind: dbus::get(&props, "DeviceType").unwrap_or(0),
            interface: dbus::string(&props, "Interface"),
            state: dbus::get(&props, "State").unwrap_or(0),
        });
    }
    Ok(out)
}

/// Saved connections: id, uuid, type, and SSID for Wi-Fi.
async fn saved(conn: &Connection) -> anyhow::Result<Vec<(String, String, String, String, String)>> {
    let settings = "/org/freedesktop/NetworkManager/Settings";
    let paths: Vec<OwnedObjectPath> =
        dbus::call(conn, NM, settings, "org.freedesktop.NetworkManager.Settings", "ListConnections", &()).await?;
    let mut out = Vec::new();
    for path in paths {
        let s: HashMap<String, HashMap<String, OwnedValue>> =
            match dbus::call(conn, NM, &path, "org.freedesktop.NetworkManager.Settings.Connection", "GetSettings", &())
                .await
            {
                Ok(s) => s,
                Err(_) => continue,
            };
        let c = s.get("connection").cloned().unwrap_or_default();
        let ssid = s
            .get("802-11-wireless")
            .and_then(|w| dbus::get::<Vec<u8>>(w, "ssid"))
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        out.push((dbus::string(&c, "id"), dbus::string(&c, "uuid"), dbus::string(&c, "type"), ssid, path.to_string()));
    }
    Ok(out)
}

fn security(props: &Props) -> (bool, String) {
    let flags: u32 = dbus::get(props, "Flags").unwrap_or(0);
    let wpa: u32 = dbus::get(props, "WpaFlags").unwrap_or(0);
    let rsn: u32 = dbus::get(props, "RsnFlags").unwrap_or(0);
    // NM_802_11_AP_SEC_KEY_MGMT_SAE marks WPA3.
    let label = if rsn & 0x400 != 0 {
        "WPA3"
    } else if rsn != 0 {
        "WPA2"
    } else if wpa != 0 {
        "WPA"
    } else if flags & 0x1 != 0 {
        "WEP"
    } else {
        ""
    };
    (!label.is_empty(), label.to_string())
}

async fn snapshot(conn: &Connection) -> anyhow::Result<State> {
    let nm = dbus::get_all(conn, NM, NM_PATH, NM).await?;
    let devices = devices(conn).await?;
    let saved = saved(conn).await.unwrap_or_default();
    let mut state = State { wifi_enabled: dbus::get(&nm, "WirelessEnabled").unwrap_or(false), ..Default::default() };

    // Active connections: the primary one, VPNs, and anything activating.
    let mut active_vpns = HashMap::new();
    for path in dbus::paths(&nm, "ActiveConnections") {
        let Ok(ac) = dbus::get_all(conn, NM, &path, "org.freedesktop.NetworkManager.Connection.Active").await else {
            continue;
        };
        let kind = dbus::string(&ac, "Type");
        let ac_state: u32 = dbus::get(&ac, "State").unwrap_or(0);
        if matches!(kind.as_str(), "vpn" | "wireguard") {
            active_vpns.insert(dbus::string(&ac, "Uuid"), ac_state);
        } else if kind == "802-11-wireless" && ac_state == 1 {
            state.activating = Some(dbus::string(&ac, "Id"));
        }
    }

    let primary_path = dbus::path(&nm, "PrimaryConnection");
    let mut primary = None;
    if let Some(path) = &primary_path
        && let Ok(ac) = dbus::get_all(conn, NM, path, "org.freedesktop.NetworkManager.Connection.Active").await
    {
        let kind = match dbus::string(&ac, "Type").as_str() {
            "802-11-wireless" => Kind::Wifi,
            "802-3-ethernet" => Kind::Ethernet,
            "vpn" | "wireguard" => Kind::Vpn,
            _ => Kind::Other,
        };
        primary = Some(Primary { name: dbus::string(&ac, "Id"), kind, strength: 0 });
    }

    for device in &devices {
        match device.kind {
            1 => {
                if state.wired.as_ref().is_none_or(|w| !w.connected) {
                    state.wired = Some(Wired { interface: device.interface.clone(), connected: device.state == 100 });
                }
            }
            2 => {
                state.wifi_device = true;
                let iface = "org.freedesktop.NetworkManager.Device.Wireless";
                let Ok(w) = dbus::get_all(conn, NM, &device.path, iface).await else { continue };
                let active_ap = dbus::path(&w, "ActiveAccessPoint");
                let mut by_ssid: HashMap<String, AccessPoint> = HashMap::new();
                for ap_path in dbus::paths(&w, "AccessPoints") {
                    let Ok(ap) = dbus::get_all(conn, NM, &ap_path, "org.freedesktop.NetworkManager.AccessPoint").await
                    else {
                        continue;
                    };
                    let ssid = dbus::get::<Vec<u8>>(&ap, "Ssid")
                        .map(|b| String::from_utf8_lossy(&b).into_owned())
                        .unwrap_or_default();
                    if ssid.is_empty() {
                        continue;
                    }
                    let (secured, security) = security(&ap);
                    let entry = AccessPoint {
                        known: saved.iter().any(|s| s.3 == ssid),
                        strength: dbus::get(&ap, "Strength").unwrap_or(0),
                        frequency: dbus::get(&ap, "Frequency").unwrap_or(0),
                        active: active_ap.as_deref() == Some(ap_path.as_str()),
                        secured,
                        security,
                        path: ap_path.clone(),
                        ssid: ssid.clone(),
                    };
                    // One row per SSID: keep the active or strongest access point.
                    let keep = by_ssid
                        .get(&ssid)
                        .is_none_or(|old| entry.active || (!old.active && entry.strength > old.strength));
                    if keep {
                        by_ssid.insert(ssid, entry);
                    }
                }
                let mut aps: Vec<_> = by_ssid.into_values().collect();
                aps.sort_by(|a, b| b.active.cmp(&a.active).then(b.strength.cmp(&a.strength)));
                if let (Some(p), Some(ap)) = (primary.as_mut(), aps.iter().find(|a| a.active))
                    && p.kind == Kind::Wifi
                {
                    p.strength = ap.strength;
                }
                state.access_points = aps;
            }
            _ => {}
        }
    }
    state.primary = primary;
    state.vpns = saved
        .iter()
        .filter(|s| matches!(s.2.as_str(), "vpn" | "wireguard"))
        .map(|s| {
            let ac = active_vpns.get(&s.1).copied();
            Vpn { name: s.0.clone(), uuid: s.1.clone(), active: ac == Some(2), connecting: ac == Some(1) }
        })
        .collect();
    Ok(state)
}

async fn wifi_device(conn: &Connection) -> anyhow::Result<String> {
    devices(conn)
        .await?
        .into_iter()
        .find(|d| d.kind == 2)
        .map(|d| d.path)
        .ok_or_else(|| anyhow::anyhow!("no Wi-Fi device"))
}

async fn apply(conn: &Connection, cmd: Cmd, state: &State) -> anyhow::Result<()> {
    match cmd {
        Cmd::SetWifi(on) => dbus::set_prop(conn, NM, NM_PATH, NM, "WirelessEnabled", Value::from(on)).await,
        Cmd::Scan => {
            let device = wifi_device(conn).await?;
            let opts: HashMap<String, Value> = HashMap::new();
            dbus::invoke(conn, NM, &device, "org.freedesktop.NetworkManager.Device.Wireless", "RequestScan", &(opts,))
                .await
        }
        Cmd::Connect(ssid) => {
            let device = wifi_device(conn).await?;
            let ap = state.access_points.iter().find(|a| a.ssid == ssid);
            let ap_path = ap.map(|a| a.path.clone()).unwrap_or("/".into());
            let saved = saved(conn).await?;
            let ap_obj = OwnedObjectPath::try_from(ap_path)?;
            let dev_obj = OwnedObjectPath::try_from(device)?;
            if let Some(known) = saved.iter().find(|s| s.3 == ssid) {
                let conn_obj = OwnedObjectPath::try_from(known.4.clone())?;
                let _: OwnedObjectPath =
                    dbus::call(conn, NM, NM_PATH, NM, "ActivateConnection", &(conn_obj, dev_obj, ap_obj)).await?;
            } else {
                // A new network. NM fills in the details from the access point, and asks a
                // registered secret agent for a password if it needs one.
                let settings: HashMap<String, HashMap<String, Value>> = HashMap::new();
                let _: (OwnedObjectPath, OwnedObjectPath) =
                    dbus::call(conn, NM, NM_PATH, NM, "AddAndActivateConnection", &(settings, dev_obj, ap_obj)).await?;
            }
            Ok(())
        }
        Cmd::Disconnect => {
            let device = wifi_device(conn).await?;
            dbus::invoke(conn, NM, &device, "org.freedesktop.NetworkManager.Device", "Disconnect", &()).await
        }
        Cmd::SetVpn(uuid, on) => {
            let nm = dbus::get_all(conn, NM, NM_PATH, NM).await?;
            if on {
                let saved = saved(conn).await?;
                let Some(vpn) = saved.iter().find(|s| s.1 == uuid) else { anyhow::bail!("no VPN {uuid}") };
                let root = OwnedObjectPath::try_from("/")?;
                let _: OwnedObjectPath = dbus::call(
                    conn,
                    NM,
                    NM_PATH,
                    NM,
                    "ActivateConnection",
                    &(OwnedObjectPath::try_from(vpn.4.clone())?, root.clone(), root),
                )
                .await?;
            } else {
                for path in dbus::paths(&nm, "ActiveConnections") {
                    let ac = dbus::get_all(conn, NM, &path, "org.freedesktop.NetworkManager.Connection.Active").await?;
                    if dbus::string(&ac, "Uuid") == uuid {
                        dbus::invoke(
                            conn,
                            NM,
                            NM_PATH,
                            NM,
                            "DeactivateConnection",
                            &(OwnedObjectPath::try_from(path)?,),
                        )
                        .await?;
                    }
                }
            }
            Ok(())
        }
    }
}

async fn run(link: &Link<State, Cmd>) -> anyhow::Result<()> {
    let conn = Connection::system().await?;
    let mut stream = dbus::signals_from(&conn, NM).await?;
    let mut state = snapshot(&conn).await?;
    link.publish(state.clone());
    loop {
        match dbus::next(&link.cmds, &mut stream, Duration::from_millis(120)).await? {
            Wake::Quit => return Ok(()),
            Wake::Changed => {}
            Wake::Command(cmd) => {
                if let Err(e) = apply(&conn, cmd, &state).await {
                    tracing::warn!("network: {e:#}");
                }
            }
        }
        state = snapshot(&conn).await?;
        link.publish(state.clone());
    }
}

fn run_fake(link: Link<State, Cmd>) {
    let ap = |ssid: &str, strength, security: &str, active, known| AccessPoint {
        ssid: ssid.into(),
        strength,
        secured: !security.is_empty(),
        security: security.into(),
        frequency: if ssid.ends_with("5G") { 5200 } else { 2437 },
        active,
        known,
        path: String::new(),
    };
    let mut state = State {
        primary: Some(Primary { name: "Home_5G".into(), kind: Kind::Wifi, strength: 82 }),
        wifi_enabled: true,
        wifi_device: true,
        wired: None,
        access_points: vec![
            ap("Home_5G", 82, "WPA3", true, true),
            ap("Observatory", 64, "WPA2", false, true),
            ap("Polaris_2G", 41, "WPA2", false, false),
            ap("Guest", 30, "", false, false),
        ],
        vpns: vec![Vpn { name: "Office VPN".into(), uuid: "fake-vpn".into(), active: false, connecting: false }],
        activating: None,
    };
    link.publish(state.clone());
    while let Ok(cmd) = link.cmds.recv_blocking() {
        match cmd {
            Cmd::SetWifi(on) => {
                state.wifi_enabled = on;
                for a in &mut state.access_points {
                    a.active = on && a.ssid == "Home_5G";
                }
                state.primary = on.then(|| Primary { name: "Home_5G".into(), kind: Kind::Wifi, strength: 82 });
            }
            Cmd::Connect(ssid) => {
                state.activating = Some(ssid.clone());
                link.publish(state.clone());
                std::thread::sleep(Duration::from_millis(900));
                state.activating = None;
                for a in &mut state.access_points {
                    a.active = a.ssid == ssid;
                    a.known |= a.active;
                }
                let strength = state.access_points.iter().find(|a| a.active).map_or(0, |a| a.strength);
                state.access_points.sort_by(|a, b| b.active.cmp(&a.active).then(b.strength.cmp(&a.strength)));
                state.primary = Some(Primary { name: ssid, kind: Kind::Wifi, strength });
            }
            Cmd::Disconnect => {
                state.access_points.iter_mut().for_each(|a| a.active = false);
                state.primary = None;
            }
            Cmd::SetVpn(uuid, on) => {
                for v in &mut state.vpns {
                    if v.uuid == uuid {
                        v.active = on;
                    }
                }
            }
            Cmd::Scan => {}
        }
        if !link.publish(state.clone()) {
            break;
        }
    }
}

pub async fn check() -> anyhow::Result<String> {
    let conn = Connection::system().await?;
    let s = snapshot(&conn).await?;
    let primary = s.primary.map(|p| p.name).unwrap_or("offline".into());
    let wifi =
        if s.wifi_device { format!("Wi-Fi {} networks", s.access_points.len()) } else { "no Wi-Fi device".into() };
    Ok(format!("NetworkManager · {primary} · {wifi} · {} VPN(s)", s.vpns.len()))
}
