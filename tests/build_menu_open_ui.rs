//! #735: opening the build menu lays the windows out once and reads item pictures back cheaply,
//! run from tests/build_menu_open_ui.cjs on the shipped web/app.js and web/room.js.
#[test]
fn build_menu_open_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/build_menu_open_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
