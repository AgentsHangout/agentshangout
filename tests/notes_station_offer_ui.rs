//! I119: the notes station offer, run from tests/notes_station_offer_ui.cjs.
#[test]
fn notes_station_offer_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/notes_station_offer_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
