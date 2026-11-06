//! BT12, BT13, BT14: the Dreaming graph's Expand, its notes panel and its names, run from
//! tests/dreaming_ui.cjs.
#[test]
fn expand_notes_panel_and_readable_names() {
    let output = std::process::Command::new("node")
        .arg("tests/dreaming_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
