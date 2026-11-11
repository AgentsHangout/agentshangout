//! S92: the Bring home button in My bots, run from tests/bring_home_ui.cjs.
#[test]
fn bring_home_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/bring_home_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
