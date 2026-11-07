//! Client app updates remain explicit and work through standard service-worker APIs.
#[test]
fn client_updates() {
    let output = std::process::Command::new("node")
        .arg("tests/client_updates_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
