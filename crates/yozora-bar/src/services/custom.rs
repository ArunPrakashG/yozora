//! `custom/<name>` modules: the output of a user command.
//!
//! With `interval`, the command runs every that many seconds and its last line is shown.
//! Without it, the command is kept running and each line it prints replaces the text, for
//! scripts that watch something themselves. A line may be plain text or JSON like
//! `{"text": "…", "tooltip": "…"}`.

use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use gpui::{App, Entity};
use yozora_config::bar::CustomConfig;

use super::{Custom, Link};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Output {
    pub text: String,
    pub tooltip: String,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub outputs: BTreeMap<String, Output>,
}

pub enum Cmd {
    Configure(BTreeMap<String, CustomConfig>),
}

pub fn start(cx: &mut App, config: BTreeMap<String, CustomConfig>) -> Entity<Custom> {
    super::spawn(cx, "custom", false, move |link| run(link, config))
}

pub fn parse_line(line: &str) -> Output {
    let line = line.trim();
    if line.starts_with('{')
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(line)
    {
        return Output {
            text: v["text"].as_str().unwrap_or_default().to_string(),
            tooltip: v["tooltip"].as_str().unwrap_or_default().to_string(),
            error: None,
        };
    }
    Output { text: line.to_string(), ..Default::default() }
}

struct Worker {
    config: CustomConfig,
    stop: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
}

impl Worker {
    fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(mut child) = self.child.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn spawn_worker(name: String, config: CustomConfig, tx: async_channel::Sender<(String, Output)>) -> Worker {
    let stop = Arc::new(AtomicBool::new(false));
    let child = Arc::new(Mutex::new(None));
    let worker = Worker { config: config.clone(), stop: stop.clone(), child: child.clone() };
    std::thread::Builder::new()
        .name(format!("yz-custom-{name}"))
        .spawn(move || {
            let send = |out: Output| tx.send_blocking((name.clone(), out)).is_ok();
            match config.interval {
                Some(secs) => {
                    let every = Duration::from_secs(secs.max(1));
                    while !stop.load(Ordering::Relaxed) {
                        let out = match yozora_ui::gpu::child(&mut Command::new("sh"))
                            .arg("-c")
                            .arg(&config.exec)
                            .stdin(Stdio::null())
                            .output()
                        {
                            Ok(o) => {
                                let text = String::from_utf8_lossy(&o.stdout);
                                let mut out =
                                    parse_line(text.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or(""));
                                if !o.status.success() {
                                    out.error = Some(format!("exited with {}", o.status));
                                }
                                out
                            }
                            Err(e) => Output { error: Some(e.to_string()), ..Default::default() },
                        };
                        if !send(out) {
                            return;
                        }
                        let mut slept = Duration::ZERO;
                        while slept < every && !stop.load(Ordering::Relaxed) {
                            std::thread::sleep(Duration::from_millis(200));
                            slept += Duration::from_millis(200);
                        }
                    }
                }
                None => {
                    // A stream that ends is started again, waiting longer each time it fails
                    // quickly, so a broken script doesn't spin.
                    let mut backoff = Duration::from_secs(1);
                    while !stop.load(Ordering::Relaxed) {
                        let started = std::time::Instant::now();
                        let spawned = yozora_ui::gpu::child(&mut Command::new("sh"))
                            .arg("-c")
                            .arg(&config.exec)
                            .stdin(Stdio::null())
                            .stdout(Stdio::piped())
                            .stderr(Stdio::null())
                            .spawn();
                        let mut c = match spawned {
                            Ok(c) => c,
                            Err(e) => {
                                send(Output { error: Some(e.to_string()), ..Default::default() });
                                return;
                            }
                        };
                        let stdout = c.stdout.take();
                        *child.lock().unwrap_or_else(|e| e.into_inner()) = Some(c);
                        if let Some(stdout) = stdout {
                            for line in BufReader::new(stdout).lines() {
                                let Ok(line) = line else { break };
                                if stop.load(Ordering::Relaxed) || !send(parse_line(&line)) {
                                    return;
                                }
                            }
                        }
                        if let Some(mut c) = child.lock().unwrap_or_else(|e| e.into_inner()).take() {
                            let _ = c.wait();
                        }
                        backoff = if started.elapsed() > Duration::from_secs(30) {
                            Duration::from_secs(1)
                        } else {
                            (backoff * 2).min(Duration::from_secs(30))
                        };
                        let mut slept = Duration::ZERO;
                        while slept < backoff && !stop.load(Ordering::Relaxed) {
                            std::thread::sleep(Duration::from_millis(200));
                            slept += Duration::from_millis(200);
                        }
                    }
                }
            }
        })
        .expect("spawning a custom module thread");
    worker
}

fn run(link: Link<State, Cmd>, config: BTreeMap<String, CustomConfig>) {
    let (tx, outputs) = async_channel::unbounded::<(String, Output)>();
    let mut workers: BTreeMap<String, Worker> = BTreeMap::new();
    let mut state = State::default();
    let configure =
        |config: BTreeMap<String, CustomConfig>, workers: &mut BTreeMap<String, Worker>, state: &mut State| {
            workers.retain(|name, w| {
                let keep = config.get(name) == Some(&w.config);
                if !keep {
                    w.stop();
                    state.outputs.remove(name);
                }
                keep
            });
            for (name, c) in config {
                if !workers.contains_key(&name) && !c.exec.trim().is_empty() {
                    workers.insert(name.clone(), spawn_worker(name, c, tx.clone()));
                }
            }
        };
    configure(config, &mut workers, &mut state);
    link.publish(state.clone());
    loop {
        enum Wake {
            Out((String, Output)),
            Cmd(Cmd),
            Quit,
        }
        let wake = async_io::block_on(futures_lite::FutureExt::or(
            async { outputs.recv().await.map(Wake::Out).unwrap_or(Wake::Quit) },
            async { link.cmds.recv().await.map(Wake::Cmd).unwrap_or(Wake::Quit) },
        ));
        match wake {
            Wake::Quit => break,
            Wake::Out((name, out)) => {
                if workers.contains_key(&name) {
                    state.outputs.insert(name, out);
                }
            }
            Wake::Cmd(Cmd::Configure(config)) => configure(config, &mut workers, &mut state),
        }
        if !link.publish(state.clone()) {
            break;
        }
    }
    for w in workers.values() {
        w.stop();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_json_and_text() {
        assert_eq!(super::parse_line("  hello ").text, "hello");
        let j = super::parse_line(r#"{"text":"3 updates","tooltip":"pacman"}"#);
        assert_eq!((j.text.as_str(), j.tooltip.as_str()), ("3 updates", "pacman"));
    }

    #[test]
    fn restarts_a_stream_that_exits() {
        let (tx, rx) = async_channel::unbounded();
        let config = super::CustomConfig { exec: "echo tick".into(), ..Default::default() };
        let worker = super::spawn_worker("t".into(), config, tx);
        // Once at start, then again after the backoff.
        for _ in 0..2 {
            let (name, out) = async_io::block_on(async {
                futures_lite::future::or(async { rx.recv().await.ok() }, async {
                    async_io::Timer::after(std::time::Duration::from_secs(5)).await;
                    None
                })
                .await
            })
            .expect("the stream should print again after it exits");
            assert_eq!((name.as_str(), out.text.as_str()), ("t", "tick"));
        }
        worker.stop();
    }
}
