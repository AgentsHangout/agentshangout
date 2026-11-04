//! S83: the setup steps for each harness in Bring a bot, run from tests/connect_steps_ui.cjs.
#[test]
fn connect_steps_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/connect_steps_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
