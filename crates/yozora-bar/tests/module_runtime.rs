#[path = "../src/services/modules.rs"]
mod modules;

use modules::*;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const GOOD_JSON: &str = r#"{"bar":{"icon":"sparkles","text":"hi"}}"#;

fn unique(tag: &str) -> PathBuf {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let d = std::env::temp_dir().join(format!("yozora-{tag}-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn make_module(root: &std::path::Path, name: &str, refresh: &str, exec: &str) -> PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let toml = format!(
        "schema = 1\nname = \"{name}\"\ntitle = \"T\"\nversion = \"0.1.0\"\nicon = \"x\"\nrefresh = {refresh}\nexec = '''{exec}'''\n"
    );
    std::fs::write(dir.join("module.toml"), toml).unwrap();
    dir
}

fn load(root: &std::path::Path, name: &str) -> ModuleDef {
    let (mods, bad) = discover(&[root.to_path_buf()]);
    assert!(bad.is_empty(), "{bad:?}");
    mods.into_iter().find(|m| m.manifest.name == name).unwrap()
}

fn recv(rx: &async_channel::Receiver<(String, Event)>) -> (String, Event) {
    let start = Instant::now();
    loop {
        if let Ok(v) = rx.try_recv() {
            return v;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "timed out waiting for event");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn push_module_invalid_line_keeps_last_good() {
    let root = unique("rt-push");
    make_module(&root, "pusher", "{ push = true }", &format!("echo '{GOOD_JSON}'; echo 'garbage'; sleep 30"));
    let (tx, rx) = async_channel::unbounded();
    let h = spawn(load(&root, "pusher"), tx);
    let (name, first) = recv(&rx);
    assert_eq!(name, "pusher");
    assert!(matches!(first, Event::Update(_)), "{first:?}");
    let (_, second) = recv(&rx);
    assert!(matches!(second, Event::Error(_)), "{second:?}");
    let mut st = ModuleState::default();
    st.apply(&first);
    st.apply(&second);
    assert!(st.last_good.is_some());
    assert!(!st.error.as_deref().unwrap().is_empty());
    h.stop();
}

#[test]
fn discover_reports_bad_manifest_and_dedups() {
    let root = unique("rt-disc");
    let root2 = unique("rt-disc2");
    make_module(&root, "good", "{ interval = 30 }", "true");
    make_module(&root2, "good", "{ interval = 60 }", "false");
    let bad = root.join("broken");
    std::fs::create_dir_all(&bad).unwrap();
    std::fs::write(bad.join("module.toml"), "schema = 1\nbogus = 2\n").unwrap();
    let (mods, errs) = discover(&[root.clone(), root2]);
    assert_eq!(mods.len(), 1);
    assert_eq!(mods[0].manifest.exec, "true");
    assert_eq!(errs.len(), 1);
    assert!(errs[0].0.ends_with("broken/module.toml"));
    assert!(!errs[0].1.is_empty());
}

#[test]
fn interval_module_works() {
    let root = unique("rt-int");
    make_module(&root, "ticker", "{ interval = 30 }", &format!("echo; echo '{GOOD_JSON}'; echo"));
    let (tx, rx) = async_channel::unbounded();
    let h = spawn(load(&root, "ticker"), tx);
    let (_, ev) = recv(&rx);
    assert!(matches!(ev, Event::Update(_)), "{ev:?}");
    h.refresh();
    let (_, ev) = recv(&rx);
    assert!(matches!(ev, Event::Update(_)), "{ev:?}");
    h.stop();
}

#[test]
fn interval_failure_is_error_with_stderr() {
    let root = unique("rt-interr");
    make_module(&root, "failing", "{ interval = 30 }", "echo boom >&2; exit 3");
    let (tx, rx) = async_channel::unbounded();
    let h = spawn(load(&root, "failing"), tx);
    match recv(&rx).1 {
        Event::Error(e) => assert!(e.contains("boom"), "{e}"),
        other => panic!("{other:?}"),
    }
    h.stop();
}
