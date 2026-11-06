//! I131: a line's references to earlier messages become links, run from tests/message_refs_ui.cjs.
#[test]
fn message_refs_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/message_refs_ui.cjs")
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
