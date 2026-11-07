//! #831: a dreaming bot in a bed has a status line, run from tests/bed_does_ui.cjs.
#[test]
fn bed_does_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/bed_does_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
