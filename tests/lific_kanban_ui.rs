//! I132: the Lific board opens as a kanban, run from tests/lific_kanban_ui.cjs.
#[test]
fn lific_kanban_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/lific_kanban_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
