#[path = "../src/services/modules.rs"]
mod modules;

use modules::*;
use std::time::Duration;

#[test]
fn backoff_grows_and_caps() {
    let t = Timing::default();
    assert_eq!(backoff(0, &t), Duration::ZERO);
    assert_eq!(backoff(1, &t), Duration::from_secs(1));
    assert_eq!(backoff(2, &t), Duration::from_secs(2));
    assert_eq!(backoff(3, &t), Duration::from_secs(4));
    assert_eq!(backoff(5, &t), Duration::from_secs(16));
    assert_eq!(backoff(6, &t), Duration::from_secs(30));
    assert_eq!(backoff(100, &t), Duration::from_secs(30));
}

#[test]
fn exiting_module_is_restarted_with_growing_gaps() {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let root = std::env::temp_dir().join(format!("yozora-restart-{}-{n}", std::process::id()));
    let dir = root.join("flaky");
    std::fs::create_dir_all(&dir).unwrap();
    // Each start appends the current time in ms (via date) to the counter file.
    let counter = root.join("counter");
    let toml = format!(
        "schema = 1\nname = \"flaky\"\ntitle = \"T\"\nversion = \"0.1.0\"\nicon = \"x\"\nrefresh = {{ push = true }}\nexec = '''date +%s%3N >> {}'''\n",
        counter.display()
    );
    std::fs::write(dir.join("module.toml"), toml).unwrap();
    let (mods, bad) = discover(std::slice::from_ref(&root));
    assert!(bad.is_empty(), "{bad:?}");
    let (tx, rx) = async_channel::unbounded();
    let h = spawn_with(
        mods.into_iter().next().unwrap(),
        tx,
        Timing { base: Duration::from_millis(20), cap: Duration::from_millis(80) },
    );
    std::thread::sleep(Duration::from_secs(1));
    h.stop();
    let starts: Vec<u128> =
        std::fs::read_to_string(&counter).unwrap().lines().map(|l| l.trim().parse().unwrap()).collect();
    assert!(starts.len() >= 3, "only {} starts", starts.len());
    assert!(starts[2] - starts[0] >= 40, "gaps did not grow: {starts:?}");
    // The consumer saw the exits as errors.
    let mut errors = 0;
    while let Ok((_, ev)) = rx.try_recv() {
        if ev == Event::Error("exited".into()) {
            errors += 1;
        }
    }
    assert!(errors >= 2);
}
