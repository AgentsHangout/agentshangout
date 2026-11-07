//! #831: guard null .charAt when opening a house/room where an item has does: null, run from tests/house_null_charat_ui.cjs.
#[test]
fn house_null_charat_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/house_null_charat_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
