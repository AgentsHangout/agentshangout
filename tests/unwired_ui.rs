//! Every UI script in `tests/*.cjs` that no other test names runs here with node, so a new script
//! is part of `cargo test` from the start. A script with its own wrapper is left to that wrapper.
use std::{fs, process::Command};

#[test]
fn ui_scripts_without_a_wrapper_run_here() {
    let mut named = String::new();
    let mut scripts = Vec::new();
    for entry in fs::read_dir("tests").unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if name.ends_with(".rs") && name != "unwired_ui.rs" {
            named.push_str(&fs::read_to_string(&path).unwrap());
        } else if name.ends_with(".cjs") {
            scripts.push(name);
        }
    }
    scripts.sort();
    let mut failed = Vec::new();
    for script in scripts.iter().filter(|name| !named.contains(&format!("tests/{name}"))) {
        let out = Command::new("node").arg(format!("tests/{script}")).output().expect("node runs");
        if !out.status.success() {
            failed.push(format!(
                "tests/{script} failed:\n{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}
