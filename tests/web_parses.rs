//! Every script in web/ has to parse. The other tests in Node cut one function out of a file, so a
//! stray quote anywhere else would pass them and still leave every page blank, as v0.1.232 did.

use std::process::Command;

#[test]
fn every_web_script_parses() {
    let dir = std::env::temp_dir().join(format!("web-parses-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut failures = Vec::new();
    for entry in std::fs::read_dir("web").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("js") {
            continue;
        }
        // As .mjs, so Node reads it as the module the browser loads.
        let copy = dir.join(path.file_stem().unwrap()).with_extension("mjs");
        std::fs::copy(&path, &copy).unwrap();
        let out = Command::new("node").arg("--check").arg(&copy).output().expect("node runs");
        if !out.status.success() {
            failures.push(format!("{}:\n{}", path.display(), String::from_utf8_lossy(&out.stderr)));
        }
    }
    std::fs::remove_dir_all(&dir).ok();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
