//! `shell.toml`, read at start and again whenever it changes. `YZ_LOOK` overrides the look, for
//! testing and the snap binary.

use std::sync::Arc;

use yozora_config::{paths, shell};

use crate::shared::Shared;

pub fn load() -> shell::ShellConfig {
    let mut c = match shell::load(&paths::config_dir()) {
        Ok(l) => {
            for w in &l.warnings {
                tracing::warn!("{w}");
            }
            l.value
        }
        Err(e) => {
            tracing::warn!("{e:#}");
            shell::ShellConfig::default()
        }
    };
    // `daybreak` names Surface tension's day sky.
    match std::env::var("YZ_LOOK").as_deref() {
        Ok("daybreak") => {
            c.look = "tension".into();
            c.time = "day".into();
        }
        Ok(look) if shell::LOOKS.contains(&look) => c.look = look.into(),
        _ => {}
    }
    if let Ok(fx) = std::env::var("YZ_FX") && shell::FX.contains(&fx.as_str()) {
        c.fx = fx;
    }
    c
}

/// Applies edits to `shell.toml` as they happen.
pub fn watch(sh: Arc<Shared>) {
    let _ = std::thread::Builder::new().name("config".into()).spawn(move || {
        let Ok(mut watcher) = yozora_config::watch::DirWatcher::new(&paths::config_dir()) else { return };
        loop {
            match futures_lite::future::block_on(watcher.changed()) {
                Ok(files) if files.iter().any(|f| f == shell::FILE) => sh.set_config(load()),
                Ok(_) => {}
                Err(_) => return,
            }
        }
    });
}

/// Writes one key of `shell.toml`, keeping the file's comments, and applies it at once.
pub fn set(sh: &Shared, key: &str, value: &str) {
    let path = paths::config_dir().join(shell::FILE);
    match yozora_config::edit::ConfigFile::open(&path).and_then(|mut f| {
        f.set(key, value.to_owned());
        f.save()
    }) {
        Ok(()) => sh.set_config(load()),
        Err(e) => tracing::warn!("{e:#}"),
    }
}
