//! W117, I152: an open map or room leaves the graphics to everything else, run from
//! tests/frame_pace_ui.mjs against the shipped web/world.js and web/room.js.
#[test]
fn an_idle_map_and_room_cap_their_frames() {
    let output = std::process::Command::new("node")
        .arg("tests/frame_pace_ui.mjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
