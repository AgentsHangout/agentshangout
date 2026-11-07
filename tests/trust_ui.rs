//! BT7, BT8, BT11, BT12: the connections graph, run from tests/trust_ui.cjs.
#[test]
fn trust_panel_and_graph() {
    let output =
        std::process::Command::new("node").arg("tests/trust_ui.cjs").output().expect("node runs");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

// BT8: every page that loads the app also loads the stylesheet the trust graph is drawn with. The
// home page at `/` once didn't, and a reload there drew the graph with no styling at all.
#[test]
fn every_page_that_loads_the_app_loads_trust_css() {
    let mut checked = 0;
    for entry in std::fs::read_dir("web").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("html") {
            continue;
        }
        let page = std::fs::read_to_string(&path).unwrap();
        if page.contains("src=\"/app.js\"") {
            assert!(
                page.contains("href=\"/trust.css\""),
                "{} loads app.js without trust.css",
                path.display()
            );
            checked += 1;
        }
    }
    assert!(checked >= 2, "expected index.html and home.html, found {checked}");
}
