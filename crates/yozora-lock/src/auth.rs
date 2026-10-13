//! The password field's state: what's typed, whether PAM is checking it, misses and waits.
//! Plain data with no UI or PAM, so it can be tested on its own.

use std::time::{Duration, Instant};

use zeroize::Zeroizing;

use crate::pam::Failure;

/// Longer passwords are cut off here; nobody types more into a lock screen.
const MAX_LEN: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Waiting for, or taking, a password.
    Idle,
    /// PAM is checking what was typed.
    Checking,
    /// The last password was wrong. Typing again goes back to `Idle`.
    Wrong,
    /// Too many misses: no tries until `wait_until`.
    Waiting,
    /// The password was right.
    Unlocked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Plain,
    Warn,
    Bad,
}

pub struct Auth {
    password: Zeroizing<String>,
    /// How many dots to show while checking, after the password has gone to PAM.
    checking_len: usize,
    pub phase: Phase,
    /// Misses since the last wait.
    misses: u32,
    /// Waits so far; each one is twice the last.
    waits: u32,
    wait_until: Option<Instant>,
    /// What PAM's modules said about the last miss.
    message: Option<String>,
    /// Counts misses, so the shake restarts for each one.
    pub shakes: u32,
    tries: u32,
    wait: Duration,
}

impl Auth {
    pub fn new(tries: u32, wait_s: u32) -> Self {
        Self {
            password: Zeroizing::new(String::new()),
            checking_len: 0,
            phase: Phase::Idle,
            misses: 0,
            waits: 0,
            wait_until: None,
            message: None,
            shakes: 0,
            tries: tries.max(1),
            wait: Duration::from_secs(wait_s.into()),
        }
    }

    fn accepts_input(&self) -> bool {
        matches!(self.phase, Phase::Idle | Phase::Wrong)
    }

    /// Adds typed text. Returns whether anything changed.
    pub fn push(&mut self, text: &str) -> bool {
        if !self.accepts_input() || text.chars().any(char::is_control) {
            return false;
        }
        if self.password.len() + text.len() > MAX_LEN {
            return false;
        }
        self.password.push_str(text);
        self.phase = Phase::Idle;
        true
    }

    pub fn backspace(&mut self) -> bool {
        if !self.accepts_input() {
            return false;
        }
        self.phase = Phase::Idle;
        self.password.pop().is_some()
    }

    pub fn clear(&mut self) -> bool {
        if !self.accepts_input() {
            return false;
        }
        self.phase = Phase::Idle;
        let had = !self.password.is_empty();
        // Zeroizing wipes on drop; clear() alone would leave the bytes in the buffer.
        self.password = Zeroizing::new(String::new());
        had
    }

    pub fn can_submit(&self) -> bool {
        self.accepts_input() && !self.password.is_empty()
    }

    /// Dots to draw in the field.
    pub fn dots(&self) -> usize {
        match self.phase {
            Phase::Checking => self.checking_len,
            _ => self.password.chars().count(),
        }
    }

    /// Takes the password for PAM and starts checking.
    pub fn submit(&mut self) -> Option<Zeroizing<String>> {
        if !self.can_submit() {
            return None;
        }
        self.checking_len = self.password.chars().count();
        self.phase = Phase::Checking;
        self.message = None;
        Some(std::mem::replace(&mut self.password, Zeroizing::new(String::new())))
    }

    pub fn succeeded(&mut self) {
        self.phase = Phase::Unlocked;
        self.misses = 0;
        self.checking_len = 0;
    }

    pub fn failed(&mut self, failure: Failure, now: Instant) {
        self.checking_len = 0;
        self.shakes += 1;
        self.misses += 1;
        self.message = (!failure.messages.is_empty()).then(|| failure.messages.join(" "));
        if failure.max_tries || self.misses >= self.tries {
            let wait = self.wait * 2u32.saturating_pow(self.waits.min(8));
            self.waits += 1;
            self.misses = 0;
            if wait.is_zero() {
                self.phase = Phase::Wrong;
            } else {
                self.wait_until = Some(now + wait);
                self.phase = Phase::Waiting;
            }
        } else {
            self.phase = Phase::Wrong;
        }
    }

    /// Ends a wait that's over. Returns whether anything changed.
    pub fn tick(&mut self, now: Instant) -> bool {
        if self.phase == Phase::Waiting && self.wait_until.is_none_or(|t| now >= t) {
            self.phase = Phase::Idle;
            self.wait_until = None;
            return true;
        }
        false
    }

    pub fn waiting_for(&self, now: Instant) -> Option<Duration> {
        (self.phase == Phase::Waiting)
            .then(|| self.wait_until.map_or(Duration::ZERO, |t| t.saturating_duration_since(now)))
    }

    /// The line under the field. `caps` is whether Caps Lock is on.
    pub fn hint(&self, caps: bool) -> (Tone, String) {
        match self.phase {
            Phase::Checking => (Tone::Plain, "Checking…".into()),
            Phase::Unlocked => (Tone::Plain, "Unlocking…".into()),
            Phase::Waiting => match &self.message {
                Some(m) => (Tone::Bad, m.clone()),
                None if self.waits > 1 => (Tone::Plain, "Too many tries · the wait doubles each time".into()),
                None => (Tone::Plain, "Too many tries".into()),
            },
            Phase::Wrong => {
                if let Some(m) = &self.message {
                    return (Tone::Bad, m.clone());
                }
                let left = self.tries - self.misses;
                // The count appears from the second miss, or when it's the last try.
                if self.misses >= 2 || left == 1 {
                    let tries = if left == 1 { "1 try".to_string() } else { format!("{left} tries") };
                    (Tone::Bad, format!("Wrong password · {tries} left"))
                } else {
                    (Tone::Bad, "Wrong password".into())
                }
            }
            Phase::Idle if caps => (Tone::Warn, "Caps Lock is on".into()),
            Phase::Idle => (Tone::Plain, "Type your password to unlock".into()),
        }
    }
}

/// `0:24`, for the wait in the field.
pub fn countdown(d: Duration) -> String {
    let s = d.as_secs() + u64::from(d.subsec_nanos() > 0);
    format!("{}:{:02}", s / 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn miss() -> Failure {
        Failure { messages: Vec::new(), max_tries: false, error: "Authentication failure".into() }
    }

    #[test]
    fn typing_submitting_and_succeeding() {
        let mut a = Auth::new(5, 30);
        assert!(!a.can_submit());
        assert!(a.push("ab"));
        assert!(a.push("ç"));
        assert!(!a.push("\n"));
        assert_eq!(a.dots(), 3);
        assert!(a.backspace());
        let pw = a.submit().unwrap();
        assert_eq!(pw.as_str(), "ab");
        assert_eq!(a.phase, Phase::Checking);
        // Still two dots while checking, and no typing.
        assert_eq!(a.dots(), 2);
        assert!(!a.push("x"));
        a.succeeded();
        assert_eq!(a.phase, Phase::Unlocked);
    }

    #[test]
    fn misses_then_a_doubling_wait() {
        let now = Instant::now();
        let mut a = Auth::new(3, 30);
        a.push("x");
        a.submit();
        a.failed(miss(), now);
        assert_eq!(a.hint(false), (Tone::Bad, "Wrong password".into()));
        a.push("x");
        a.submit();
        a.failed(miss(), now);
        assert_eq!(a.hint(false), (Tone::Bad, "Wrong password · 1 try left".into()));
        a.push("x");
        a.submit();
        a.failed(miss(), now);
        assert_eq!(a.phase, Phase::Waiting);
        assert!(!a.push("x"));
        assert_eq!(countdown(a.waiting_for(now).unwrap()), "0:30");
        assert!(!a.tick(now + Duration::from_secs(29)));
        assert!(a.tick(now + Duration::from_secs(30)));
        assert_eq!(a.phase, Phase::Idle);

        // The next wait is twice as long.
        for _ in 0..3 {
            a.push("x");
            a.submit();
            a.failed(miss(), now);
        }
        assert_eq!(countdown(a.waiting_for(now).unwrap()), "1:00");
        assert_eq!(a.hint(false).1, "Too many tries · the wait doubles each time");
    }

    #[test]
    fn pam_messages_and_max_tries() {
        let now = Instant::now();
        let mut a = Auth::new(5, 30);
        a.push("x");
        a.submit();
        let locked = Failure {
            messages: vec!["The account is locked due to 3 failed logins.".into()],
            max_tries: true,
            error: String::new(),
        };
        a.failed(locked, now);
        assert_eq!(a.phase, Phase::Waiting);
        assert_eq!(a.hint(false), (Tone::Bad, "The account is locked due to 3 failed logins.".into()));
    }

    #[test]
    fn caps_lock_hint_only_while_idle() {
        let mut a = Auth::new(5, 30);
        assert_eq!(a.hint(true).0, Tone::Warn);
        a.push("x");
        a.submit();
        assert_eq!(a.hint(true).1, "Checking…");
    }
}
