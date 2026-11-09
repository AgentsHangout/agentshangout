//! #1005: the core menu entries are one list, in one order, on the map and in a room; run from
//! tests/shared_menu_ui.cjs against the shipped web/app.js.
#[test]
fn the_core_menu_entries_come_in_one_order_on_the_map_and_in_a_room() {
    let output = std::process::Command::new("node")
        .arg("tests/shared_menu_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
