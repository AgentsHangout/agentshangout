//! I148: a room line shows bold, italic and code, run from tests/chat_marks_ui.cjs.
#[test]
fn chat_marks_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/chat_marks_ui.cjs")
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
