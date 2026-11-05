//! #529: the build catalog paints each picture once and copies it, run from tests/build_pictures_ui.cjs.
#[test]
fn build_pictures_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/build_pictures_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
