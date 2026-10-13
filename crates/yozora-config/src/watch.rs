//! Watches a directory for file changes with inotify.
//!
//! Editors and Settings replace files by renaming a temp file over them, so we watch the directory
//! rather than the file. If the directory doesn't exist yet we watch its parent and start watching
//! it once it's created.

use std::{
    ffi::OsString,
    os::fd::{AsFd, OwnedFd},
    path::{Path, PathBuf},
};

use async_io::Async;
use inotify::{Inotify, WatchDescriptor, WatchMask};

pub struct DirWatcher {
    dir: PathBuf,
    inotify: Inotify,
    readable: Async<OwnedFd>,
    dir_watch: Option<WatchDescriptor>,
    parent_watch: Option<WatchDescriptor>,
    buffer: Vec<u8>,
}

const FILE_MASK: WatchMask = WatchMask::CLOSE_WRITE
    .union(WatchMask::MOVED_TO)
    .union(WatchMask::MOVED_FROM)
    .union(WatchMask::CREATE)
    .union(WatchMask::DELETE)
    .union(WatchMask::DELETE_SELF)
    .union(WatchMask::MOVE_SELF);

impl DirWatcher {
    pub fn new(dir: &Path) -> anyhow::Result<Self> {
        let inotify = Inotify::init()?;
        let readable = Async::new(inotify.as_fd().try_clone_to_owned()?)?;
        let mut watcher = Self {
            dir: dir.to_path_buf(),
            inotify,
            readable,
            dir_watch: None,
            parent_watch: None,
            buffer: vec![0; 4096],
        };
        watcher.arm()?;
        Ok(watcher)
    }

    fn arm(&mut self) -> anyhow::Result<()> {
        if self.dir.is_dir() {
            self.dir_watch = Some(self.inotify.watches().add(&self.dir, FILE_MASK)?);
            if let Some(parent) = self.parent_watch.take() {
                let _ = self.inotify.watches().remove(parent);
            }
        } else if let Some(parent) = self.dir.parent().filter(|p| p.is_dir()) {
            self.parent_watch = Some(
                self.inotify
                    .watches()
                    .add(parent, WatchMask::CREATE | WatchMask::MOVED_TO)?,
            );
        }
        Ok(())
    }

    /// Waits for changes and returns the names of the files that changed.
    pub async fn changed(&mut self) -> anyhow::Result<Vec<OsString>> {
        loop {
            self.readable.readable().await?;
            let mut names = Vec::new();
            let mut rearm = false;
            match self.inotify.read_events(&mut self.buffer) {
                Ok(events) => {
                    for event in events {
                        if Some(&event.wd) == self.parent_watch.as_ref() {
                            if event.name.is_some_and(|n| Some(n) == self.dir.file_name()) {
                                rearm = true;
                            }
                        } else if event.mask.intersects(
                            inotify::EventMask::DELETE_SELF | inotify::EventMask::MOVE_SELF,
                        ) {
                            self.dir_watch = None;
                            rearm = true;
                        } else if let Some(name) = event.name {
                            names.push(name.to_os_string());
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => continue,
                Err(e) => return Err(e.into()),
            }
            if rearm {
                self.arm()?;
                // A new directory may already hold files.
                if self.dir_watch.is_some() {
                    names.push("*".into());
                }
            }
            if !names.is_empty() {
                names.sort();
                names.dedup();
                return Ok(names);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sees_renames_and_late_directories() {
        let root = std::env::temp_dir().join(format!("yozora-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let dir = root.join("yozora");
        let mut watcher = DirWatcher::new(&dir).unwrap();

        async_io::block_on(async {
            std::fs::create_dir(&dir).unwrap();
            let names = watcher.changed().await.unwrap();
            assert!(names.contains(&"*".into()));

            std::fs::write(dir.join("bar.toml.tmp"), "x").unwrap();
            std::fs::rename(dir.join("bar.toml.tmp"), dir.join("bar.toml")).unwrap();
            let mut seen = Vec::new();
            while !seen.contains(&OsString::from("bar.toml")) {
                seen.extend(watcher.changed().await.unwrap());
            }
        });
        let _ = std::fs::remove_dir_all(&root);
    }
}
