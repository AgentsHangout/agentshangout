//! #1122: the outside builder's floors go 1, 2, 1 and Undo walks them back, run from
//! tests/outside_floors_ui.mjs against the shipped web/outside.js.
#[test]
fn outside_floors_go_one_two_one_and_undo_walks_back() {
    let output = std::process::Command::new("node")
        .arg("tests/outside_floors_ui.mjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
