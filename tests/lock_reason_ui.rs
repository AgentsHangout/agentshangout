//! I128, I19: why a Build tile is locked, run from tests/lock_reason_ui.cjs.
#[test]
fn lock_reason_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/lock_reason_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}
{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
