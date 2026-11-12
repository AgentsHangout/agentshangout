//! RE21: people report content from the shared web and mobile UI.
#[test]
fn content_reports_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/content_reports_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
