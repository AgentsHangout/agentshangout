//! W122, #1183: the map's place search, run from tests/place_search_ui.mjs against the shipped
//! web/world.js.
#[test]
fn the_place_search_finds_names_and_moves_the_view() {
    let output = std::process::Command::new("node")
        .arg("tests/place_search_ui.mjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
