//! HC54: the credits glance, run from tests/credits_glance_ui.cjs.
#[test]
fn credits_glance_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/credits_glance_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
