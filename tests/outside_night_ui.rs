//! #567: the outside view opens as the map looks, run from tests/outside_night_ui.cjs.
#[test]
fn outside_night_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/outside_night_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
