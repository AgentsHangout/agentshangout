//! #837: a bunk bed has a sleeping spot for each bunk, run from tests/bunk_bed_ui.cjs.
#[test]
fn bunk_bed_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/bunk_bed_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
