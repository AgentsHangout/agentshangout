//! #913: the passkey, plan invite and bot setup code load when asked for, run from tests/lazy_modules_ui.cjs.
#[test]
fn lazy_modules_ui() {
    let output = std::process::Command::new("node")
        .arg("tests/lazy_modules_ui.cjs")
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "{}
{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
