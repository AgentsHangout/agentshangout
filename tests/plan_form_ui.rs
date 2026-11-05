//! F56: the plan form, run from tests/plan_form_ui.cjs.
#[test]
fn plan_form_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/plan_form_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
