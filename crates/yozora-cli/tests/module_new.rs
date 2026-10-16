use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn new_scaffold_passes_check_and_lists() {
    let tmp = std::env::temp_dir().join(format!("yozora-modnew-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let bin = env!("CARGO_BIN_EXE_yozora");
    let t = tmp.to_str().unwrap();

    let o = Command::new(bin).args(["module", "new", "demo", "--dir", t]).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let demo = tmp.join("demo");
    assert!(demo.join("module.toml").is_file());
    assert!(std::fs::metadata(demo.join("run.sh")).unwrap().permissions().mode() & 0o111 != 0);

    let o = Command::new(bin).args(["module", "check", demo.to_str().unwrap()]).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stdout).contains("ok: demo 0.1.0"));

    let o = Command::new(bin).args(["module", "new", "demo", "--dir", t]).output().unwrap();
    assert!(!o.status.success());
    let o = Command::new(bin).args(["module", "new", "Bad_Name", "--dir", t]).output().unwrap();
    assert!(!o.status.success());

    let o = Command::new(bin).args(["module", "list"]).env("YOZORA_MODULE_PATH", t).output().unwrap();
    assert!(o.status.success());
    assert!(String::from_utf8_lossy(&o.stdout).contains("demo"));
}
