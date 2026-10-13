//! XDG locations.

use std::{env, path::PathBuf};

fn home() -> PathBuf {
    env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    env::var_os(var).map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| home().join(fallback))
}

/// `~/.config/yozora`, or `$YOZORA_CONFIG_DIR` when set (tests, `--config`).
pub fn config_dir() -> PathBuf {
    env::var_os("YOZORA_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| xdg("XDG_CONFIG_HOME", ".config").join("yozora"))
}

/// `$XDG_RUNTIME_DIR/yozora`, where control sockets live.
pub fn runtime_dir() -> PathBuf {
    let base = env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(env::temp_dir);
    base.join("yozora")
}

/// The socket for app `id`.
pub fn socket_path(id: &str) -> PathBuf {
    runtime_dir().join(format!("{id}.sock"))
}

/// Data directories in priority order: `$XDG_DATA_HOME`, then each of `$XDG_DATA_DIRS`.
pub fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![xdg("XDG_DATA_HOME", ".local/share")];
    let system = env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    dirs.extend(system.split(':').filter(|s| !s.is_empty()).map(PathBuf::from));
    dirs
}

/// Where app manifests are installed, in priority order.
pub fn manifest_dirs() -> Vec<PathBuf> {
    data_dirs().into_iter().map(|d| d.join("yozora/apps")).collect()
}
