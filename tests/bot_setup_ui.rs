//! #647 B: the bot setup approval page and its Needs you item, run from tests/bot_setup_ui.cjs.
#[test]
fn bot_setup_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/bot_setup_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
