//! #548: the grouped @ picker, run from tests/tag_picker_ui.cjs.
#[test]
fn tag_picker_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/tag_picker_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
