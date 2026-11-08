//! What's playing and how many notifications arrived while locked, read from the shell's
//! services. Only a count of notifications is exposed; their contents never reach the lock
//! screen.

use std::{path::PathBuf, sync::Mutex};

use yozora_services::{
    Media as MediaService, Notifications as NotificationsService,
    media::{self, Status},
};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Media {
    pub title: String,
    pub artist: String,
    /// A local file.
    pub art: Option<PathBuf>,
    pub playing: bool,
    bus: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Glance {
    pub media: Option<Media>,
    /// Notifications that arrived since locking, if a notification server is running.
    pub notifications: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaCmd {
    Previous,
    PlayPause,
    Next,
}

pub(crate) struct Sources {
    media: Option<MediaService>,
    notifications: Option<NotificationsService>,
    /// The newest notification id when the lock started; later ones count.
    first_id: Mutex<Option<u32>>,
}

impl Sources {
    pub fn start(media: bool, notifications: bool) -> Self {
        Self {
            media: media.then(|| media::start(false)),
            notifications: notifications.then(|| yozora_services::notifications::start(false)),
            first_id: Mutex::new(None),
        }
    }

    pub fn glance(&self) -> Glance {
        Glance { media: self.media.as_ref().and_then(current), notifications: self.count() }
    }

    pub fn command(&self, cmd: MediaCmd) {
        let (Some(service), Some(media)) = (&self.media, self.media.as_ref().and_then(current)) else { return };
        // Aim at the player the lock screen shows, which may not be the service's own pick.
        service.send(media::Cmd::Select(media.bus));
        service.send(match cmd {
            MediaCmd::Previous => media::Cmd::Previous,
            MediaCmd::PlayPause => media::Cmd::PlayPause,
            MediaCmd::Next => media::Cmd::Next,
        });
    }

    /// Notifications with an id above the newest one there was at the first reading.
    fn count(&self) -> Option<usize> {
        let service = self.notifications.as_ref()?;
        if !service.up() {
            return None;
        }
        let snapshot = service.read();
        let ids: Vec<u32> = snapshot.state.items.iter().map(|n| n.id).collect();
        let mut first = self.first_id.lock().unwrap_or_else(|e| e.into_inner());
        let first = *first.get_or_insert_with(|| ids.iter().copied().max().unwrap_or(0));
        Some(ids.iter().filter(|id| **id > first).count())
    }
}

/// The playing player, else the first paused one.
fn current(service: &MediaService) -> Option<Media> {
    let snapshot = service.read();
    let players = &snapshot.state.players;
    let shown = |status| players.iter().find(|p| p.status == status && !p.title.is_empty());
    let p = shown(Status::Playing).or_else(|| shown(Status::Paused))?;
    Some(Media {
        title: p.title.clone(),
        artist: p.artist.clone(),
        art: p.art.clone(),
        playing: p.status == Status::Playing,
        bus: p.bus.clone(),
    })
}
