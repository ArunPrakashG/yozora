use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

static N: AtomicUsize = AtomicUsize::new(0);

fn tmp() -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "yozora-modcheck-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn yozora(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_yozora")).args(args).output().unwrap()
}

const GOOD: &str = "schema = 1\nname = \"demo\"\ntitle = \"Demo\"\nversion = \"0.1.0\"\nicon = \"x\"\nrefresh = { interval = 30 }\nexec = \"true\"\n";
const GOOD_LINE: &str =
    r#"{"bar":{"icon":"x","text":"hi"},"popover":{"title":"T","body":[{"type":"note","text":"n"}]}}"#;

#[test]
fn unknown_field_names_field_and_line() {
    let d = tmp();
    std::fs::write(d.join("module.toml"), format!("{GOOD}colour = \"red\"\n")).unwrap();
    let o = yozora(&["module", "check", d.to_str().unwrap()]);
    assert!(!o.status.success());
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("module.toml:8"), "{err}");
    assert!(err.contains("colour"), "{err}");
}

#[test]
fn interval_below_minimum_fails() {
    let d = tmp();
    std::fs::write(d.join("module.toml"), GOOD.replace("interval = 30", "interval = 1")).unwrap();
    let o = yozora(&["module", "check", d.to_str().unwrap()]);
    assert!(!o.status.success());
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("interval") && err.contains("module.toml:6"), "{err}");
}

#[test]
fn good_manifest_and_sample_pass() {
    let d = tmp();
    std::fs::write(d.join("module.toml"), GOOD).unwrap();
    let s = d.join("sample.jsonl");
    std::fs::write(&s, format!("{GOOD_LINE}\n")).unwrap();
    let o = yozora(&["module", "check", d.to_str().unwrap(), "--sample", s.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stdout).contains("ok:"));
}

#[test]
fn sample_with_color_field_fails_with_line() {
    let d = tmp();
    std::fs::write(d.join("module.toml"), GOOD).unwrap();
    let s = d.join("sample.jsonl");
    let bad = r#"{"bar":{"icon":"x","color":"red"}}"#;
    std::fs::write(&s, format!("{GOOD_LINE}\n{bad}\n")).unwrap();
    let o = yozora(&["module", "check", d.to_str().unwrap(), "--sample", s.to_str().unwrap()]);
    assert!(!o.status.success());
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("sample:2") && err.contains("color"), "{err}");
}
