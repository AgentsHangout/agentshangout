//! Functional room objects use the existing panels and permissions.
#[test]
fn room_object_controls() {
    let output = std::process::Command::new("node")
        .arg("tests/room_objects_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
