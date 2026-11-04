//! RP13: the plan card's Repeat control and per-bot yes, run from tests/repeat_ui.cjs.
#[test]
fn repeat_ui() {
    let output =
        std::process::Command::new("node").arg("tests/repeat_ui.cjs").output().expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
