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

/// #1185, W122: a place nobody named is called "<owner>'s <Title>" with the title from
/// `spaces::place_title`, and the search drops the owner only before a title in `PLACE_TITLES` in
/// web/world.js. If a kind's title is missing there, its places become findable by their owner's
/// name, so the two lists must hold the same titles. Town Square has no owner part (W82).
#[test]
fn the_search_knows_every_title_an_unnamed_place_gets() {
    let server = include_str!("../src/spaces.rs");
    let body = server
        .split("pub fn place_title(kind: &str) -> &'static str {")
        .nth(1)
        .and_then(|rest| rest.split("\n}").next())
        .expect("src/spaces.rs has place_title");
    let titles: std::collections::BTreeSet<&str> = body
        .split("=> \"")
        .skip(1)
        .filter_map(|arm| arm.split('"').next())
        .filter(|title| *title != "Town Square")
        .collect();
    let page = include_str!("../web/world.js");
    let list = page
        .split("const PLACE_TITLES = [")
        .nth(1)
        .and_then(|rest| rest.split("];").next())
        .expect("web/world.js has PLACE_TITLES");
    let searched: std::collections::BTreeSet<&str> = list.split('\'').skip(1).step_by(2).collect();
    assert!(titles.len() > 5, "read too few titles from place_title: {titles:?}");
    assert_eq!(
        searched, titles,
        "PLACE_TITLES in web/world.js is out of step with spaces::place_title, so an unnamed place's owner would be searchable"
    );
}
