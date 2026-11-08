//! #830 UI: the map's "Report a problem" button, its mapping and the served module.
#[test]
fn bug_report_button_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/bug_report_button_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
