//! #807: the room's furniture and bots draw in front-edge order, and the desk and table block the rects they're drawn in.
//! Runs tests/room_depth_ui.mjs against the shipped web/room.js.
#[test]
fn room_depth_and_solid_cells() {
    let output = std::process::Command::new("node")
        .arg("tests/room_depth_ui.mjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
