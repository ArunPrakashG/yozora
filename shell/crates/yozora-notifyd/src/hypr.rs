//! Hyprland's event socket, for knowing when the screen is being shared.

/// Calls `changed` whenever Hyprland starts or stops sharing the screen, from its event socket,
/// until `changed` returns false. Reconnects if Hyprland restarts; returns at once outside
/// Hyprland.
pub fn watch_screencast(mut changed: impl FnMut(bool) -> bool) {
    use std::io::BufRead;
    let Ok(sig) = std::env::var("HYPRLAND_INSTANCE_SIGNATURE") else { return };
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).unwrap_or_else(|| "/tmp".into());
    let path = runtime.join("hypr").join(sig).join(".socket2.sock");
    let mut sharing = false;
    loop {
        if let Ok(stream) = std::os::unix::net::UnixStream::connect(&path) {
            for line in std::io::BufReader::new(stream).lines() {
                let Ok(line) = line else { break };
                if let Some(on) = screencast(&line)
                    && on != sharing
                {
                    sharing = on;
                    if !changed(on) {
                        return;
                    }
                }
            }
        }
        // The socket went away: sharing ended with it.
        if sharing {
            sharing = false;
            if !changed(false) {
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
}

/// `screencast>>1,0` → started.
fn screencast(line: &str) -> Option<bool> {
    let rest = line.strip_prefix("screencast>>")?;
    Some(rest.split(',').next()? == "1")
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_screencast_events() {
        assert_eq!(super::screencast("screencast>>1,0"), Some(true));
        assert_eq!(super::screencast("screencast>>0,1"), Some(false));
        assert_eq!(super::screencast("workspace>>2"), None);
    }
}
