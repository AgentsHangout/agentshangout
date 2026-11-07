#[test]
fn game_windows_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/game_windows_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
