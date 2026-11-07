//! CR10: where a car is on the server's route, run from tests/drive_route_ui.cjs.
#[test]
fn drive_route_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/drive_route_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
