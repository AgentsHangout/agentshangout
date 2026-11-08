//! #830 UI: the bug report control sends only fixed codes and a capped note.
#[test]
fn bug_report_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/bug_report_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
