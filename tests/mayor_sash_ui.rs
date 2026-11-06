//! MY10: the mayor's sash on the page, run from tests/mayor_sash_ui.cjs.
#[test]
fn mayor_sash_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/mayor_sash_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
