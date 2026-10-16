use yozora_module::Manifest;

const GOOD: &str = r#"
schema = 1
name = "claude-sessions"
title = "Claude"
version = "0.1.0"
icon = "sparkles"
refresh = { interval = 30 }
exec = "claude-sessions"

[[actions]]
id = "kill"
label = "Stop"
destructive = true
"#;

#[test]
fn good_parses() {
    let m = Manifest::parse(GOOD).unwrap();
    assert_eq!(m.actions.len(), 1);
    assert!(m.actions[0].destructive);
}

#[test]
fn push_parses() {
    let s = GOOD.replace("{ interval = 30 }", "{ push = true }");
    Manifest::parse(&s).unwrap();
}

#[test]
fn unknown_field_names_field() {
    let e = Manifest::parse(&format!("{GOOD}\nbogus = 1\n")).unwrap_err();
    assert!(e.to_string().contains("bogus"), "{e}");
}

#[test]
fn wrong_schema_rejected() {
    let s = GOOD.replace("schema = 1", "schema = 2");
    assert!(Manifest::parse(&s).is_err());
}

#[test]
fn short_interval_rejected() {
    let s = GOOD.replace("interval = 30", "interval = 4");
    assert!(Manifest::parse(&s).is_err());
}

#[test]
fn bad_name_rejected() {
    let s = GOOD.replace("claude-sessions\"\ntitle", "Claude_X\"\ntitle");
    assert!(Manifest::parse(&s).is_err());
}
