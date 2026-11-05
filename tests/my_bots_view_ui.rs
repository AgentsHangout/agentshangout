//! S86: one My bots button and full-screen view, run from tests/my_bots_view_ui.cjs.
#[test]
fn my_bots_view_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/my_bots_view_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
