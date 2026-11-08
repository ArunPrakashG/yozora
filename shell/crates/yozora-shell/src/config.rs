//! `shell.toml`, read at start and again whenever it changes. `YZ_LOOK` overrides the look, for
//! testing and the snap binary.

use std::sync::mpsc::Sender;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use yozora_config::{paths, shell};

use crate::shared::Shared;

/// Edits sent to the writer and not yet on disk. The watcher waits them out, so it never reads
/// the file half way through a burst of clicks and puts an older look back.
static PENDING: AtomicUsize = AtomicUsize::new(0);

pub fn load() -> shell::ShellConfig {
    let c = match shell::load(&paths::config_dir()) {
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
    overrides(c)
}

/// Applies the `YZ_LOOK` and `YZ_FX` overrides.
fn overrides(mut c: shell::ShellConfig) -> shell::ShellConfig {
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
                Ok(files) if files.iter().any(|f| f == shell::FILE) => {
                    if PENDING.load(Ordering::Acquire) == 0 {
                        sh.set_config(load());
                    }
                }
                Ok(_) => {}
                Err(_) => return,
            }
        }
    });
}

/// The look and effects a config means right now: Surface tension turns to Daybreak by day.
pub fn resolve(c: &shell::ShellConfig) -> (yozora_style::Look, yozora_style::Fx) {
    use yozora_style::{Fx, Look};
    let fx = Fx::from_slug(&c.fx).unwrap_or(Fx::Full);
    let look = Look::from_slug(&c.look).unwrap_or_default();
    let day = match c.time.as_str() {
        "day" => true,
        "follow" => {
            use chrono::Timelike;
            let now = chrono::Local::now();
            c.is_day(now.hour() * 60 + now.minute())
        }
        _ => false,
    };
    (if look == Look::Tension && day { Look::Daybreak } else { look }, fx)
}

/// Writes one key of `shell.toml`, keeping the file's comments, and applies it at once.
pub fn set(sh: &Shared, key: &str, value: &str) {
    set_many(sh, &[(key, value)]);
}

/// Several keys as one change: the shell applies them in one redraw, and the file is written
/// behind the click, so an fsync never holds up a frame.
pub fn set_many(sh: &Shared, edits: &[(&str, &str)]) {
    let mut c = sh.config();
    for (key, value) in edits {
        match *key {
            "look" => c.look = (*value).into(),
            "time" => c.time = (*value).into(),
            "fx" => c.fx = (*value).into(),
            "day_from" => c.day_from = (*value).into(),
            "night_from" => c.night_from = (*value).into(),
            _ => {}
        }
    }
    sh.set_config(overrides(c));
    let batch: Vec<(String, String)> = edits.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
    PENDING.fetch_add(1, Ordering::AcqRel);
    if writer().send(batch).is_err() {
        PENDING.fetch_sub(1, Ordering::AcqRel);
        tracing::warn!("shell.toml writer is gone");
    }
}

/// The thread that writes `shell.toml`, so edits land in order and a burst of clicks is one write.
fn writer() -> &'static Sender<Vec<(String, String)>> {
    static WRITER: OnceLock<Sender<Vec<(String, String)>>> = OnceLock::new();
    WRITER.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Vec<(String, String)>>();
        let _ = std::thread::Builder::new().name("config-write".into()).spawn(move || {
            while let Ok(mut edits) = rx.recv() {
                let mut batches = 1;
                while let Ok(more) = rx.try_recv() {
                    edits.extend(more);
                    batches += 1;
                }
                let path = paths::config_dir().join(shell::FILE);
                let saved = yozora_config::edit::ConfigFile::open(&path).and_then(|mut f| {
                    for (k, v) in &edits {
                        f.set(k, v.clone());
                    }
                    f.save()
                });
                if let Err(e) = saved {
                    tracing::warn!("{e:#}");
                }
                PENDING.fetch_sub(batches, Ordering::AcqRel);
            }
        });
        tx
    })
}
