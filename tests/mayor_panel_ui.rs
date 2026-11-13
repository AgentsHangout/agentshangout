//! MY1 to MY11, #639, #1158: the Mayor voting box panel and badge, run from tests/mayor_panel_ui.cjs.
#[test]
fn mayor_panel_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/mayor_panel_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
