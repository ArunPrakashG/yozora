//! Asks Jev (TypeSafe AI) how much a notification needs attention now. Off unless `[jev]` in
//! `notify.toml` is on with a key. The toast never waits for it: the answer arrives a moment
//! later, and then a toast that can wait leaves early, and during Do Not Disturb one that can't
//! is shown.
//!
//! Requests go through `curl`, with the key on its standard input so it never shows in the
//! process list.

use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    io::Write,
    process::{Command, Stdio},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use serde_json::{Value, json};
use yozora_config::notify::JevSection;

use crate::store::{Notification, Urgency};

const ASK: &str = "This is a notification on someone's desktop. How likely is it that they would want to see \
it right away, rather than read it later? Likely: a person writing to them directly, a call, a meeting or \
reminder that is due now, something that failed or needs them to act, a security alert. Unlikely: marketing \
and promotions, newsletters, news and recommendations, social media activity, and routine confirmations such \
as a finished download, an installed update or a completed sync.";

/// Requests in flight at once; more are skipped rather than queued.
const MAX_IN_FLIGHT: usize = 4;
/// Answers remembered, by content.
const MAX_SEEN: usize = 2000;

static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);
static SEEN: Mutex<Option<HashMap<u64, f64>>> = Mutex::new(None);

/// Whether `n` may be sent. Critical ones always show, and progress, volume-style popups and
/// transient ones are never worth asking about.
pub fn eligible(n: &Notification, cfg: &JevSection) -> bool {
    if !cfg.active() || n.transient || n.tag.is_some() || n.progress.is_some() || n.urgency == Urgency::Critical {
        return false;
    }
    if n.summary.trim().is_empty() && n.body.trim().is_empty() {
        return false;
    }
    !cfg.skip_apps.iter().any(|a| {
        let a = a.trim();
        !a.is_empty() && (a.eq_ignore_ascii_case(&n.app) || a.eq_ignore_ascii_case(&n.desktop_entry))
    })
}

/// What the answer is for: a replaced notification with new content is asked about again.
pub fn fingerprint(n: &Notification, send_body: bool) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (&n.app, &n.summary, if send_body { n.body.as_str() } else { "" }).hash(&mut h);
    h.finish()
}

pub fn body(n: &Notification, send_body: bool) -> Value {
    let mut state = json!({
        "app": n.app,
        "title": n.summary,
        "urgency": match n.urgency { Urgency::Low => "low", Urgency::Normal => "normal", Urgency::Critical => "critical" },
    });
    if send_body && !n.body.trim().is_empty() {
        state["body"] = json!(n.body.chars().take(1000).collect::<String>());
    }
    if !n.category.is_empty() {
        state["category"] = json!(n.category);
    }
    json!({
        "state": { "notification": state },
        "model": "jev-latest",
        "questions": { "attention": { "type": "noul", "instructions": ASK } },
    })
}

/// The odds from Jev's `answers`.
pub fn odds(answers: &Value) -> Option<f64> {
    answers.pointer("/attention/noul")?.as_f64().filter(|p| (0.0..=1.0).contains(p))
}

/// Asks about `n` on a thread of its own and calls `done` with the odds. Remembered answers
/// come back at once; nothing is called if Jev can't be asked.
pub fn ask(n: &Notification, cfg: &JevSection, done: impl FnOnce(u64, f64) + Send + 'static) {
    let print = fingerprint(n, cfg.send_body);
    if let Some(p) = SEEN.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(&print).copied()) {
        done(print, p);
        return;
    }
    if IN_FLIGHT.fetch_add(1, Ordering::SeqCst) >= MAX_IN_FLIGHT {
        IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
        tracing::debug!("jev: busy; not asking");
        return;
    }
    let request = body(n, cfg.send_body);
    let (url, key) = (cfg.url.clone(), cfg.api_key.trim().to_string());
    let spawned = std::thread::Builder::new().name("yz-jev".into()).spawn(move || {
        let answer = post(&url, &key, &request).and_then(|a| odds(&a));
        IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
        if let Some(p) = answer {
            {
                let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
                let seen = seen.get_or_insert_with(HashMap::new);
                if seen.len() >= MAX_SEEN {
                    seen.clear();
                }
                seen.insert(print, p);
            }
            done(print, p);
        }
    });
    if spawned.is_err() {
        IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
    }
}

/// One request, and once more a moment later if Jev is busy. Jev's `answers`, or None.
fn post(url: &str, key: &str, request: &Value) -> Option<Value> {
    for wait in [0u64, 400] {
        std::thread::sleep(Duration::from_millis(wait));
        let (status, reply) = match curl(url, key, &request.to_string()) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("jev: {e:#}");
                return None;
            }
        };
        if status == 429 || status == 529 {
            continue;
        }
        if !(200..300).contains(&status) {
            tracing::warn!("jev: HTTP {status}");
            return None;
        }
        return serde_json::from_str::<Value>(&reply).ok()?.get("answers").cloned();
    }
    tracing::warn!("jev: busy");
    None
}

/// A quoted string for curl's config syntax.
fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace(['\n', '\r'], ""))
}

fn config(url: &str, key: &str, data: &str) -> String {
    [
        format!("url = {}", quote(url)),
        format!("header = {}", quote(&format!("Authorization: Bearer {key}"))),
        "header = \"Content-Type: application/json\"".into(),
        format!("data-binary = {}", quote(data)),
        "max-time = 5".into(),
        "silent".into(),
        "show-error".into(),
        "write-out = \"\\n%{http_code}\"".into(),
    ]
    .join("\n")
}

fn curl(url: &str, key: &str, data: &str) -> anyhow::Result<(u16, String)> {
    let mut child = Command::new("curl")
        .args(["--config", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| anyhow::anyhow!("running curl: {e}"))?;
    child.stdin.take().ok_or_else(|| anyhow::anyhow!("curl's stdin"))?.write_all(config(url, key, data).as_bytes())?;
    let out = child.wait_with_output()?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let (reply, code) = text.rsplit_once('\n').unwrap_or(("", &text));
    match code.trim().parse::<u16>() {
        Ok(0) | Err(_) => anyhow::bail!("{}", String::from_utf8_lossy(&out.stderr).trim()),
        Ok(code) => Ok((code, reply.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(app: &str, summary: &str) -> Notification {
        Notification { app: app.into(), summary: summary.into(), body: "See you at 5".into(), ..Default::default() }
    }

    fn on() -> JevSection {
        JevSection { enabled: true, api_key: "k".into(), skip_apps: vec!["KeePassXC".into()], ..Default::default() }
    }

    #[test]
    fn who_is_asked() {
        let cfg = on();
        assert!(eligible(&note("Signal", "Sam"), &cfg));
        assert!(!eligible(&note("keepassxc", "Copied"), &cfg), "skipped apps, ignoring case");
        assert!(!eligible(&Notification { urgency: Urgency::Critical, ..note("x", "y") }, &cfg));
        assert!(!eligible(&Notification { transient: true, ..note("x", "y") }, &cfg));
        assert!(!eligible(&Notification { tag: Some("volume".into()), ..note("x", "y") }, &cfg));
        assert!(!eligible(&Notification { progress: Some(40), ..note("x", "y") }, &cfg));
        assert!(!eligible(&note("Signal", "Sam"), &JevSection { api_key: " ".into(), ..cfg.clone() }));
        assert!(!eligible(&note("Signal", "Sam"), &JevSection { enabled: false, ..cfg }));
    }

    #[test]
    fn request_and_answer() {
        let n = note("Signal", "Sam");
        let b = body(&n, true);
        assert_eq!(b["state"]["notification"]["title"], "Sam");
        assert_eq!(b["state"]["notification"]["body"], "See you at 5");
        assert_eq!(b["questions"]["attention"]["type"], "noul");
        assert!(body(&n, false)["state"]["notification"].get("body").is_none(), "the body stays home");
        assert_ne!(fingerprint(&n, true), fingerprint(&Notification { body: "later".into(), ..n.clone() }, true));
        assert_eq!(fingerprint(&n, false), fingerprint(&Notification { body: "later".into(), ..n }, false));
        assert_eq!(odds(&json!({"attention": {"noul": 0.82}})), Some(0.82));
        assert_eq!(odds(&json!({"attention": {"noul": 3.0}})), None);
        assert_eq!(odds(&json!({})), None);
    }

    #[test]
    fn curl_config_quotes() {
        let c = config("https://x/y", "a\"b", r#"{"t":"say \"hi\"\\n"}"#);
        assert!(c.contains(r#"header = "Authorization: Bearer a\"b""#), "{c}");
        assert!(c.contains(r#"data-binary = "{\"t\":\"say \\\"hi\\\"\\\\n\"}""#), "{c}");
        assert!(c.contains(r#"write-out = "\n%{http_code}""#));
    }
}
