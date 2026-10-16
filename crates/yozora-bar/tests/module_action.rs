#[path = "../src/services/modules.rs"]
mod modules;

use modules::*;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn unique(tag: &str) -> PathBuf {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let d = std::env::temp_dir().join(format!("yozora-{tag}-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn setup(root: &Path, exec: &str, actions: &str) -> ModuleDef {
    let dir = root.join("acting");
    std::fs::create_dir_all(&dir).unwrap();
    let toml = format!(
        "schema = 1\nname = \"acting\"\ntitle = \"T\"\nversion = \"0.1.0\"\nicon = \"x\"\nrefresh = {{ push = true }}\nexec = '''{exec}'''\n{actions}"
    );
    std::fs::write(dir.join("module.toml"), toml).unwrap();
    let (mods, bad) = discover(&[root.to_path_buf()]);
    assert!(bad.is_empty(), "{bad:?}");
    mods.into_iter().next().unwrap()
}

fn wait_file(p: &Path, pred: impl Fn(&str) -> bool) -> String {
    let start = Instant::now();
    loop {
        if let Ok(s) = std::fs::read_to_string(p)
            && pred(&s)
        {
            return s;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "timed out waiting for {}", p.display());
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn action_reaches_process_as_one_json_line() {
    let root = unique("act");
    let out = root.join("received");
    let def = setup(
        &root,
        &format!("while read l; do echo \"$l\" >> {}; done", out.display()),
        "[[actions]]\nid = \"toggle-wifi\"\nlabel = \"Wifi\"\n\n[[actions]]\nid = \"kill\"\nlabel = \"Kill\"\ndestructive = true\n",
    );
    let (tx, _rx) = async_channel::unbounded();
    let h = spawn(def, tx);
    // Give the process a moment to start so stdin is connected.
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(h.send_action("toggle-wifi", Some(serde_json::json!(true)), false), ActionOutcome::Sent);
    assert_eq!(h.send_action("kill", None, false), ActionOutcome::NeedsConfirm);
    assert_eq!(h.send_action("nope", None, true), ActionOutcome::Unknown);
    let got = wait_file(&out, |s| s.ends_with('\n'));
    std::thread::sleep(Duration::from_millis(200));
    let got2 = std::fs::read_to_string(&out).unwrap();
    assert_eq!(got, got2);
    assert_eq!(got.lines().count(), 1, "{got}");
    let msg: yozora_module::HostMessage = serde_json::from_str(got.trim()).unwrap();
    assert_eq!(
        msg,
        yozora_module::HostMessage::Action { id: "toggle-wifi".into(), value: Some(serde_json::json!(true)) }
    );
    assert_eq!(h.send_action("kill", None, true), ActionOutcome::Sent);
    let got = wait_file(&out, |s| s.lines().count() >= 2);
    assert!(got.lines().nth(1).unwrap().contains("\"kill\""));
    h.stop();
}

#[test]
fn command_action_runs_with_env() {
    let root = unique("actcmd");
    let out = root.join("env");
    let def = setup(
        &root,
        "sleep 30",
        &format!(
            "[[actions]]\nid = \"go\"\nlabel = \"Go\"\ncommand = '''echo \"$YOZORA_ACTION|$YOZORA_VALUE|$YOZORA_MODULE\" > {}'''\n\n[[actions]]\nid = \"none\"\nlabel = \"N\"\n",
            out.display()
        ),
    );
    let (tx, _rx) = async_channel::unbounded();
    let h = spawn(def, tx);
    assert_eq!(h.send_action("go", Some(serde_json::json!(7)), false), ActionOutcome::Sent);
    let got = wait_file(&out, |s| s.ends_with('\n'));
    assert_eq!(got.trim(), "go|7|acting");
    h.stop();
}
