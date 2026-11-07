//! HC36, HC37, HC43: a card's contributions follow the server's per-caller actions.
#[test]
fn contribution_controls() {
    let output = std::process::Command::new("node")
        .arg("tests/contribution_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
