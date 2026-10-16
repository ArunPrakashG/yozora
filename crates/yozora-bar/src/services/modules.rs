//! Module discovery, supervision, output validation and action routing, with no GPUI in it.
#![allow(dead_code)] // unused until the bar renders modules

use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use yozora_module::{HostMessage, Manifest, Refresh, Update};

const INTERVAL_RUN_TIMEOUT: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(10);

/// A discovered module.
#[derive(Debug, Clone)]
pub struct ModuleDef {
    pub dir: PathBuf,
    pub manifest: Manifest,
}

/// User directory first, then the system one.
pub fn default_dirs() -> Vec<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    let mut dirs = Vec::new();
    if let Some(c) = config {
        dirs.push(c.join("yozora").join("modules"));
    }
    dirs.push(PathBuf::from("/usr/share/yozora/modules"));
    dirs
}

/// Find every subdirectory holding a `module.toml`. Earlier directories win on name clashes.
pub fn discover(dirs: &[PathBuf]) -> (Vec<ModuleDef>, Vec<(PathBuf, String)>) {
    let mut found = Vec::new();
    let mut bad = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for root in dirs {
        let Ok(rd) = std::fs::read_dir(root) else { continue };
        let mut subs: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect();
        subs.sort();
        for dir in subs {
            let file = dir.join("module.toml");
            if !file.is_file() {
                continue;
            }
            let parsed = std::fs::read_to_string(&file)
                .map_err(|e| e.to_string())
                .and_then(|s| Manifest::parse(&s).map_err(|e| e.to_string()));
            match parsed {
                Ok(manifest) => {
                    if seen.insert(manifest.name.clone()) {
                        found.push(ModuleDef { dir, manifest });
                    }
                }
                Err(e) => bad.push((file, e)),
            }
        }
    }
    (found, bad)
}

/// What a module reported.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Update(Update),
    Error(String),
}

/// Per-module state owned by the consumer.
#[derive(Debug, Clone, Default)]
pub struct ModuleState {
    pub last_good: Option<Update>,
    pub error: Option<String>,
}

impl ModuleState {
    pub fn apply(&mut self, event: &Event) {
        match event {
            Event::Update(u) => {
                self.last_good = Some(u.clone());
                self.error = None;
            }
            Event::Error(e) => self.error = Some(e.clone()),
        }
    }
}

/// Restart delay settings.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub base: Duration,
    pub cap: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Timing { base: Duration::from_secs(1), cap: Duration::from_secs(30) }
    }
}

/// `base * 2^(failures-1)`, capped; zero failures means no delay.
pub fn backoff(failures: u32, timing: &Timing) -> Duration {
    if failures == 0 {
        return Duration::ZERO;
    }
    let factor = 1u32.checked_shl(failures - 1).unwrap_or(u32::MAX);
    timing.base.checked_mul(factor).map_or(timing.cap, |d| d.min(timing.cap))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionOutcome {
    Sent,
    NeedsConfirm,
    Unknown,
    NoHandler,
}

enum Cmd {
    Refresh,
    Stop,
}

struct Shared {
    stop: AtomicBool,
    child: Mutex<Option<Child>>,
    stdin: Mutex<Option<ChildStdin>>,
}

/// Control handle for one running module.
pub struct Handle {
    def: ModuleDef,
    shared: Arc<Shared>,
    cmds: mpsc::Sender<Cmd>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Handle {
    pub fn send_action(&self, id: &str, value: Option<serde_json::Value>, confirmed: bool) -> ActionOutcome {
        let Some(decl) = self.def.manifest.actions.iter().find(|a| a.id == id) else {
            return ActionOutcome::Unknown;
        };
        if decl.destructive && !confirmed {
            return ActionOutcome::NeedsConfirm;
        }
        if let Some(cmd) = &decl.command {
            let mut c = Command::new("sh");
            c.arg("-c")
                .arg(cmd)
                .current_dir(&self.def.dir)
                .env("YOZORA_MODULE", &self.def.manifest.name)
                .env("YOZORA_ACTION", id)
                .env("YOZORA_VALUE", value.as_ref().map(|v| v.to_string()).unwrap_or_default())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            match c.spawn() {
                Ok(mut child) => {
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                }
                Err(e) => tracing::warn!("module {}: action command failed to start: {e}", self.def.manifest.name),
            }
            return ActionOutcome::Sent;
        }
        if matches!(self.def.manifest.refresh, Refresh::Push(_)) {
            let msg = HostMessage::Action { id: id.to_string(), value };
            if self.write_host(&msg) {
                return ActionOutcome::Sent;
            }
        }
        ActionOutcome::NoHandler
    }

    fn write_host(&self, msg: &HostMessage) -> bool {
        let Ok(mut line) = serde_json::to_string(msg) else { return false };
        line.push('\n');
        let mut guard = lock(&self.shared.stdin);
        match guard.as_mut() {
            Some(stdin) => match stdin.write_all(line.as_bytes()).and_then(|_| stdin.flush()) {
                Ok(()) => true,
                Err(e) => {
                    tracing::warn!("module {}: write to stdin failed: {e}", self.def.manifest.name);
                    false
                }
            },
            None => false,
        }
    }

    pub fn refresh(&self) {
        match self.def.manifest.refresh {
            Refresh::Interval(_) => {
                let _ = self.cmds.send(Cmd::Refresh);
            }
            Refresh::Push(_) => {
                self.write_host(&HostMessage::Refresh);
            }
        }
    }

    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        let _ = self.cmds.send(Cmd::Stop);
        if let Some(child) = lock(&self.shared.child).as_mut() {
            let _ = child.kill();
        }
        *lock(&self.shared.stdin) = None;
    }
}

pub fn spawn(def: ModuleDef, events: async_channel::Sender<(String, Event)>) -> Handle {
    spawn_with(def, events, Timing::default())
}

pub fn spawn_with(def: ModuleDef, events: async_channel::Sender<(String, Event)>, timing: Timing) -> Handle {
    let shared = Arc::new(Shared { stop: AtomicBool::new(false), child: Mutex::new(None), stdin: Mutex::new(None) });
    let (tx, rx) = mpsc::channel();
    let handle = Handle { def: def.clone(), shared: shared.clone(), cmds: tx };
    let name = def.manifest.name.clone();
    let res = std::thread::Builder::new().name(format!("module-{name}")).spawn(move || match def.manifest.refresh {
        Refresh::Interval(secs) => run_interval(&def, secs, &events, &shared, &rx),
        Refresh::Push(_) => run_push(&def, &events, &shared, &rx, &timing),
    });
    if let Err(e) = res {
        tracing::error!("module {name}: cannot start supervisor thread: {e}");
    }
    handle
}

fn command(def: &ModuleDef) -> Command {
    let mut c = Command::new("sh");
    c.arg("-c").arg(&def.manifest.exec).current_dir(&def.dir).env("YOZORA_MODULE", &def.manifest.name);
    c
}

/// Parse and validate one line. A valid update is returned as `Event::Update`.
fn parse_event(line: &str) -> Event {
    match Update::parse_line(line) {
        Err(e) => Event::Error(format!("invalid output: {e}")),
        Ok(u) => match u.popover.as_ref().map(|p| p.validate()) {
            Some(Err(errs)) => Event::Error(format!("invalid popover: {}", errs.join("; "))),
            _ => Event::Update(u),
        },
    }
}

/// Returns false when the consumer is gone.
fn emit(events: &async_channel::Sender<(String, Event)>, name: &str, ev: Event) -> bool {
    events.send_blocking((name.to_string(), ev)).is_ok()
}

fn last_line(s: &str) -> Option<&str> {
    s.lines().rev().map(str::trim).find(|l| !l.is_empty())
}

fn read_all<R: Read + Send + 'static>(r: Option<R>) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut r) = r {
            let _ = r.read_to_end(&mut buf);
        }
        String::from_utf8_lossy(&buf).into_owned()
    })
}

/// Sleep up to `d`, returning early (true) on stop and handling a refresh command.
/// Returns Some(true) to run now, Some(false) when stopping, None when time elapsed.
fn wait_cmd(rx: &mpsc::Receiver<Cmd>, d: Duration, shared: &Shared) -> Option<bool> {
    if shared.stop.load(Ordering::SeqCst) {
        return Some(false);
    }
    match rx.recv_timeout(d) {
        Ok(Cmd::Refresh) => Some(true),
        Ok(Cmd::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => Some(false),
        Err(mpsc::RecvTimeoutError::Timeout) => None,
    }
}

fn run_interval_once(def: &ModuleDef, shared: &Shared) -> Event {
    let mut c = command(def);
    c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match c.spawn() {
        Ok(c) => c,
        Err(e) => return Event::Error(format!("cannot start: {e}")),
    };
    let out = read_all(child.stdout.take());
    let err = read_all(child.stderr.take());
    *lock(&shared.child) = Some(child);
    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        let polled = match lock(&shared.child).as_mut() {
            Some(c) => c.try_wait(),
            None => break None,
        };
        match polled {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {}
            Err(_) => break None,
        }
        if shared.stop.load(Ordering::SeqCst) {
            break None;
        }
        if started.elapsed() >= INTERVAL_RUN_TIMEOUT {
            timed_out = true;
            if let Some(c) = lock(&shared.child).as_mut() {
                let _ = c.kill();
            }
            break None;
        }
        std::thread::sleep(POLL);
    };
    if let Some(mut c) = lock(&shared.child).take() {
        let _ = c.kill();
        let _ = c.wait();
    }
    let out = out.join().unwrap_or_default();
    let err = err.join().unwrap_or_default();
    if timed_out {
        return Event::Error("timed out".into());
    }
    let Some(status) = status else { return Event::Error("stopped".into()) };
    if !status.success() {
        let msg = last_line(&err).map(str::to_string).unwrap_or_else(|| format!("exited with {status}"));
        return Event::Error(msg);
    }
    match last_line(&out) {
        Some(line) => parse_event(line),
        None => Event::Error(last_line(&err).map(str::to_string).unwrap_or_else(|| "no output".into())),
    }
}

fn run_interval(
    def: &ModuleDef,
    secs: u64,
    events: &async_channel::Sender<(String, Event)>,
    shared: &Shared,
    rx: &mpsc::Receiver<Cmd>,
) {
    let name = &def.manifest.name;
    loop {
        if shared.stop.load(Ordering::SeqCst) {
            return;
        }
        let ev = run_interval_once(def, shared);
        if shared.stop.load(Ordering::SeqCst) {
            return;
        }
        if let Event::Error(e) = &ev {
            tracing::warn!("module {name}: {e}");
        }
        if !emit(events, name, ev) {
            return;
        }
        if let Some(false) = wait_cmd(rx, Duration::from_secs(secs), shared) {
            return;
        }
    }
}

fn run_push(
    def: &ModuleDef,
    events: &async_channel::Sender<(String, Event)>,
    shared: &Shared,
    rx: &mpsc::Receiver<Cmd>,
    timing: &Timing,
) {
    let name = &def.manifest.name;
    let mut failures: u32 = 0;
    loop {
        if shared.stop.load(Ordering::SeqCst) {
            return;
        }
        let mut c = command(def);
        c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        let mut got_valid = false;
        match c.spawn() {
            Ok(mut child) => {
                let stdout = child.stdout.take();
                *lock(&shared.stdin) = child.stdin.take();
                *lock(&shared.child) = Some(child);
                // The stop flag may have been set before the child was registered.
                if shared.stop.load(Ordering::SeqCst)
                    && let Some(c) = lock(&shared.child).as_mut()
                {
                    let _ = c.kill();
                }
                if let Some(stdout) = stdout {
                    for line in BufReader::new(stdout).lines() {
                        let Ok(line) = line else { break };
                        if line.trim().is_empty() {
                            continue;
                        }
                        let ev = parse_event(&line);
                        match &ev {
                            Event::Update(_) => got_valid = true,
                            Event::Error(e) => tracing::warn!("module {name}: {e}"),
                        }
                        if !emit(events, name, ev) {
                            shared.stop.store(true, Ordering::SeqCst);
                            break;
                        }
                        if shared.stop.load(Ordering::SeqCst) {
                            break;
                        }
                    }
                }
                *lock(&shared.stdin) = None;
                if let Some(mut c) = lock(&shared.child).take() {
                    let _ = c.kill();
                    let _ = c.wait();
                }
            }
            Err(e) => {
                tracing::warn!("module {name}: cannot start: {e}");
                if !emit(events, name, Event::Error(format!("cannot start: {e}"))) {
                    return;
                }
            }
        }
        if shared.stop.load(Ordering::SeqCst) {
            return;
        }
        if got_valid {
            failures = 0;
        } else {
            failures = failures.saturating_add(1);
        }
        if !emit(events, name, Event::Error("exited".into())) {
            return;
        }
        let delay = backoff(failures.max(1), timing);
        tracing::info!("module {name}: restarting in {delay:?}");
        let deadline = Instant::now() + delay;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            match wait_cmd(rx, left, shared) {
                Some(false) => return,
                // Refresh while down: the restart is already pending.
                Some(true) => {}
                None => break,
            }
        }
    }
}
