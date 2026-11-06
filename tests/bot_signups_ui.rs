//! A114, #628: the god-mode panel for bot sign-ups, run from tests/bot_signups_ui.cjs.
#[test]
fn bot_signups_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/bot_signups_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
