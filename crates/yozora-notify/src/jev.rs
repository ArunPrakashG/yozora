//! Asks Jev (TypeSafe AI) about a notification: how much it needs attention now, whether it's
//! private, what kind it is and whether it asks for something. All in one request. Off unless
//! `[jev]` in `notify.toml` is on with a key. The toast never waits for it: the answers arrive a
//! moment later, and then a toast that can wait leaves early, during Do Not Disturb one that
//! can't is shown, and while the screen is shared a private one hides its content.
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

use crate::store::{Answer, Habit, KINDS, Notification, Urgency};

const ASK: &str = "This is a notification on someone's desktop. How likely is it that they would want to see \
it right away, rather than read it later? Likely: a person writing to them directly, a call, a meeting or \
reminder that is due now, something that failed or needs them to act, a security alert. Unlikely: marketing \
and promotions, newsletters, news and recommendations, social media activity, and routine confirmations such \
as a finished download, an installed update or a completed sync. If state.habits is there, it says how often \
they opened or dismissed this app's earlier notifications without opening them; lean on it.";

const ASK_PRIVATE: &str = "Would they mind if someone looking at their screen, say while they share it in a \
meeting, saw this notification? Likely: login and verification codes, passwords, personal messages from friends, \
family or a partner, health, money and bank details, anything intimate. Unlikely: work alerts and build results, \
calendar reminders, app updates, newsletters and promotions.";

const ASK_ACTION: &str = "Does this notification ask them to do something themselves, such as reply, approve, \
pay, review, sign or fix something? Information that needs nothing from them, like a finished download or a news \
story, does not.";

const ASK_KIND: &str = "What kind of notification is this?";

/// Descriptions of [`KINDS`], in the same order.
const KIND_CRITERIA: [&str; 5] = [
    "A person writing to them directly: a chat, an email, a mention or a call",
    "Something failed, broke, is at risk or needs attention: errors, security warnings, low battery",
    "Something due at a time: meetings, events, timers and to-dos",
    "Routine status from an app or the system: finished downloads, installed updates, syncs and backups",
    "Marketing, newsletters, news, recommendations and social media activity",
];

/// Requests in flight at once; more are skipped rather than queued.
const MAX_IN_FLIGHT: usize = 4;
/// Answers remembered, by content.
const MAX_SEEN: usize = 2000;

static IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);
static SEEN: Mutex<Option<HashMap<u64, Answer>>> = Mutex::new(None);

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

pub fn body(n: &Notification, send_body: bool, habit: Option<Habit>) -> Value {
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
    let mut state = json!({ "notification": state });
    if let Some(h) = habit {
        state["habits"] = json!({ "opened": h.opened, "dismissed_without_opening": h.dismissed });
    }
    let criteria: serde_json::Map<String, Value> =
        KINDS.iter().zip(KIND_CRITERIA).map(|(k, d)| (k.to_string(), json!(d))).collect();
    json!({
        "state": state,
        "model": "jev-latest",
        "questions": {
            "attention": { "type": "noul", "instructions": ASK },
            "private": { "type": "noul", "instructions": ASK_PRIVATE },
            "action": { "type": "noul", "instructions": ASK_ACTION },
            "kind": { "type": "choice", "instructions": ASK_KIND, "criteria": criteria },
        },
    })
}

fn noul(answers: &Value, name: &str) -> Option<f64> {
    answers.pointer(&format!("/{name}/noul"))?.as_f64().filter(|p| (0.0..=1.0).contains(p))
}

/// Jev's `answers`, read. Nothing without the attention odds; the rest may be missing.
pub fn answer(answers: &Value) -> Option<Answer> {
    Some(Answer {
        attention: noul(answers, "attention")?,
        private: noul(answers, "private").map(|p| p >= 0.5),
        needs_action: noul(answers, "action").map(|p| p >= 0.5),
        kind: answers.pointer("/kind/choice").and_then(Value::as_str).filter(|k| KINDS.contains(k)).map(String::from),
    })
}

/// Asks about `n` on a thread of its own and calls `done` with the answers, or None if Jev
/// couldn't be asked. Remembered answers come back at once.
pub fn ask(
    n: &Notification,
    cfg: &JevSection,
    habit: Option<Habit>,
    done: impl FnOnce(u64, Option<Answer>) + Send + 'static,
) {
    let print = fingerprint(n, cfg.send_body);
    let seen = SEEN.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(&print).cloned());
    if let Some(a) = seen {
        done(print, Some(a));
        return;
    }
    if IN_FLIGHT.fetch_add(1, Ordering::SeqCst) >= MAX_IN_FLIGHT {
        IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
        tracing::debug!("jev: busy; not asking");
        done(print, None);
        return;
    }
    let request = body(n, cfg.send_body, habit.filter(|_| cfg.learn));
    let (url, key) = (cfg.url.clone(), cfg.api_key.trim().to_string());
    let spawned = std::thread::Builder::new().name("yz-jev".into()).spawn(move || {
        let answer = post(&url, &key, &request).and_then(|a| answer(&a));
        IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
        if let Some(a) = &answer {
            let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
            let seen = seen.get_or_insert_with(HashMap::new);
            if seen.len() >= MAX_SEEN {
                seen.clear();
            }
            seen.insert(print, a.clone());
        }
        done(print, answer);
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
        let b = body(&n, true, None);
        assert_eq!(b["state"]["notification"]["title"], "Sam");
        assert_eq!(b["state"]["notification"]["body"], "See you at 5");
        assert_eq!(b["questions"]["attention"]["type"], "noul");
        assert_eq!(b["questions"]["kind"]["criteria"].as_object().unwrap().len(), KINDS.len());
        assert!(b["state"].get("habits").is_none());
        assert!(body(&n, false, None)["state"]["notification"].get("body").is_none(), "the body stays home");
        let h = body(&n, true, Some(Habit { opened: 1, dismissed: 9 }));
        assert_eq!(h["state"]["habits"]["dismissed_without_opening"], 9);
        assert_ne!(fingerprint(&n, true), fingerprint(&Notification { body: "later".into(), ..n.clone() }, true));
        assert_eq!(fingerprint(&n, false), fingerprint(&Notification { body: "later".into(), ..n }, false));
        let a = answer(&json!({
            "attention": {"noul": 0.82}, "private": {"noul": 0.7}, "action": {"noul": 0.2},
            "kind": {"choice": "message", "confidence": 0.9},
        }))
        .unwrap();
        assert_eq!(
            a,
            Answer { attention: 0.82, private: Some(true), needs_action: Some(false), kind: Some("message".into()) }
        );
        let partial = answer(&json!({"attention": {"noul": 0.1}, "kind": {"choice": "spam"}})).unwrap();
        assert_eq!((partial.private, partial.kind), (None, None));
        assert_eq!(answer(&json!({"attention": {"noul": 3.0}})), None);
        assert_eq!(answer(&json!({})), None);
    }

    #[test]
    fn curl_config_quotes() {
        let c = config("https://x/y", "a\"b", r#"{"t":"say \"hi\"\\n"}"#);
        assert!(c.contains(r#"header = "Authorization: Bearer a\"b""#), "{c}");
        assert!(c.contains(r#"data-binary = "{\"t\":\"say \\\"hi\\\"\\\\n\"}""#), "{c}");
        assert!(c.contains(r#"write-out = "\n%{http_code}""#));
    }

    /// Asks the real Jev about a few notifications: `JEV_API_KEY=… cargo test -p yozora-notify
    /// live -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs JEV_API_KEY and the network"]
    fn live() {
        let key = std::env::var("JEV_API_KEY").expect("JEV_API_KEY");
        let url = JevSection::default().url;
        let samples = [
            ("Signal", "Sam", "Are you still coming at 5? I'm outside", None),
            ("Messages", "Your code", "Your Acme Bank login code is 482913. Don't share it.", None),
            ("Slack", "Deploy failed", "prod-api: rollout stuck, 3/8 pods crashlooping", None),
            ("Calendar", "Standup in 1 minute", "Zoom link inside", None),
            ("GitHub", "Review requested", "arun requested your review on #42", None),
            ("Gmail", "Weekly newsletter", "Ten deals you don't want to miss", None),
            ("Steam", "Summer Sale", "Up to 90% off", None),
            ("Discover", "Updates installed", "4 packages were updated", None),
            // Someone who always opens this app's notifications, and someone who never does.
            ("Slack", "New message in #random", "lunch?", Some(Habit { opened: 30, dismissed: 1 })),
            ("Slack", "New message in #random", "lunch?", Some(Habit { opened: 0, dismissed: 40 })),
        ];
        for (app, title, text, habit) in samples {
            let n = Notification { app: app.into(), summary: title.into(), body: text.into(), ..Default::default() };
            let started = std::time::Instant::now();
            let a = post(&url, &key, &body(&n, true, habit)).and_then(|a| answer(&a));
            let shown = a.as_ref().map_or("-".into(), |a| {
                format!(
                    "{:.2}  private {:<5}  action {:<5}  {:<9}",
                    a.attention,
                    a.private.map_or("-".into(), |p| p.to_string()),
                    a.needs_action.map_or("-".into(), |p| p.to_string()),
                    a.kind.as_deref().unwrap_or("-"),
                )
            });
            let habit = habit.map_or(String::new(), |h| format!(" (opened {}, dismissed {})", h.opened, h.dismissed));
            println!("{:>5} ms  {shown}  {app}: {title}{habit}", started.elapsed().as_millis());
            assert!(a.is_some(), "no answer for {title}");
        }
    }
}
