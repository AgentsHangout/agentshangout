//! W112, #643 B: the follow strip, run from tests/social_strip_ui.cjs.
#[test]
fn social_strip_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/social_strip_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
