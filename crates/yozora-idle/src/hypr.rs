//! Turning the monitors off and on through Hyprland's request socket.

use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    time::Duration,
};

fn request(command: &str) -> anyhow::Result<String> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|| "/tmp".into());
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE")?;
    let mut stream = UnixStream::connect(runtime.join("hypr").join(sig).join(".socket.sock"))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(command.as_bytes())?;
    let mut reply = String::new();
    stream.read_to_string(&mut reply)?;
    Ok(reply)
}

/// Whether Hyprland is there to talk to.
pub fn available() -> bool {
    std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
}

/// Turns every monitor off or on. Hyprland 0.56 takes Lua here; older ones take `dpms off`.
pub fn dpms(on: bool) {
    let action = if on { "on" } else { "off" };
    let lua = format!("dispatch hl.dsp.dpms({{ action = \"{action}\" }})");
    match request(&lua) {
        Ok(r) if r.trim() == "ok" => return,
        Ok(r) => tracing::debug!("Lua dpms refused ({}); trying the classic form", r.trim()),
        Err(e) => tracing::warn!("dpms {action}: {e:#}"),
    }
    match request(&format!("dispatch dpms {action}")) {
        Ok(r) if r.trim() == "ok" => {}
        Ok(r) => tracing::warn!("dpms {action}: {}", r.trim()),
        Err(e) => tracing::warn!("dpms {action}: {e:#}"),
    }
}
