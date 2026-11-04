//! Guards against two merge accidents: a rule id claimed twice in docs/, and conflict markers left
//! in committed files (issue 464). Std only; it reads the working tree from the crate root.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

/// Every file under `dir` (recursively) whose extension is in `exts`, sorted.
fn files_under(dir: &Path, exts: &[&str], out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files_under(&path, exts, out);
        } else if path.extension().and_then(|e| e.to_str()).is_some_and(|e| exts.contains(&e)) {
            out.push(path);
        }
    }
    out.sort();
}

fn relative(path: &Path) -> String {
    path.strip_prefix(ROOT)
        .unwrap_or(path)
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

/// The fence a line opens or closes: its character (` or ~), its length, and whether anything but
/// whitespace follows it. Up to three spaces of indent are allowed, as in CommonMark.
fn fence(line: &str) -> Option<(char, usize, bool)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let mark = rest.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let length = rest.chars().take_while(|c| *c == mark).count();
    (length >= 3).then(|| (mark, length, !rest[length..].trim().is_empty()))
}

/// The line numbers (1-based) and text of a markdown file outside fenced code blocks. A fence
/// closes only on the same character, at least as long as the opener, with nothing else on the line.
fn prose_lines(text: &str) -> Vec<(usize, &str)> {
    let mut open: Option<(char, usize)> = None;
    let mut lines = Vec::new();
    for (index, line) in text.lines().enumerate() {
        match (open, fence(line)) {
            (None, Some((mark, length, _))) => open = Some((mark, length)),
            (Some((mark, length)), Some((m, l, text_after)))
                if m == mark && l >= length && !text_after =>
            {
                open = None
            }
            (None, None) => lines.push((index + 1, line)),
            _ => {}
        }
    }
    lines
}

/// The rule id a line opens with: `- **I101.**` or `- **S19a.**`. Ids are capital letters, then
/// digits, then at most one lowercase letter. Lines like `- **MA1. Title.**` have no dot straight
/// after the id and are named differently, so they are not rule lines.
fn rule_id(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("- **")?;
    let letters = rest.bytes().take_while(u8::is_ascii_uppercase).count();
    let digits = rest[letters..].bytes().take_while(u8::is_ascii_digit).count();
    if letters == 0 || digits == 0 {
        return None;
    }
    let mut end = letters + digits;
    if rest.as_bytes().get(end).is_some_and(u8::is_ascii_lowercase) {
        end += 1;
    }
    rest[end..].starts_with(".**").then(|| &rest[..end])
}

/// Docs that number their rules apart from the rest, because they borrowed a letter another doc
/// already used (K in boards.md and bonds.md, E in events.md and outside.md). An id only has to be
/// unique within its own doc here. Don't add to this list: give a new rule a letter nobody uses.
const OWN_NUMBERING: [&str; 4] = ["boards.md", "bonds.md", "events.md", "outside.md"];

/// Design docs propose rules and restate the ones they would change, so they don't claim ids.
fn is_design_doc(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with("-design.md"))
}

#[test]
fn no_rule_id_is_claimed_twice_in_docs() {
    let mut docs = Vec::new();
    files_under(&Path::new(ROOT).join("docs"), &["md"], &mut docs);
    assert!(!docs.is_empty(), "found no docs/*.md");
    let mut seen: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for path in docs.iter().filter(|p| !is_design_doc(p)) {
        let text = fs::read_to_string(path).unwrap();
        for (number, line) in prose_lines(&text) {
            if let Some(id) = rule_id(line) {
                let file = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                let scope = if OWN_NUMBERING.contains(&file) { file } else { "" };
                seen.entry(format!(
                    "{id}{}",
                    if scope.is_empty() { String::new() } else { format!(" (in {scope})") }
                ))
                .or_default()
                .push(format!("{}:{number}", relative(path)));
            }
        }
    }
    assert!(seen.len() > 100, "the parser found only {} rule ids", seen.len());
    let doubles: Vec<String> = seen
        .iter()
        .filter(|(_, places)| places.len() > 1)
        .map(|(id, places)| format!("{id} is at {}", places.join(" and ")))
        .collect();
    assert!(
        doubles.is_empty(),
        "rule ids claimed twice (give the later one a free id, and fix what cites it):\n{}",
        doubles.join("\n")
    );
}

#[test]
fn rule_id_parser_reads_the_rule_line_shapes() {
    assert_eq!(rule_id("- **I101.** text"), Some("I101"));
    assert_eq!(rule_id("- **S19a.** text"), Some("S19a"));
    assert_eq!(rule_id("- **FR3.**"), Some("FR3"));
    assert_eq!(rule_id("- **MA1. Resources capability.** text"), None);
    assert_eq!(rule_id("  - **I101.** indented"), None);
    assert_eq!(rule_id("- **bold** text"), None);
    assert_eq!(rule_id("- **I.** no digits"), None);
}

#[test]
fn no_conflict_markers_are_committed() {
    let this_file = relative(&Path::new(ROOT).join("tests").join("rule_ids.rs"));
    let mut found = Vec::new();
    for dir in ["src", "docs", "web", "tests"] {
        let mut files = Vec::new();
        files_under(
            &Path::new(ROOT).join(dir),
            &["rs", "md", "js", "cjs", "html", "css", "sql", "json", "toml", "txt", "sh"],
            &mut files,
        );
        for path in files {
            let name = relative(&path);
            if name == this_file {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else { continue };
            let lines: Vec<(usize, &str)> = if name.ends_with(".md") {
                prose_lines(&text)
            } else {
                text.lines().enumerate().map(|(i, l)| (i + 1, l)).collect()
            };
            for (number, line) in lines {
                if line.starts_with("<<<<<<< ") || line.starts_with(">>>>>>> ") || line == "======="
                {
                    found.push(format!("{name}:{number}: {line}"));
                }
            }
        }
    }
    assert!(found.is_empty(), "conflict markers committed:\n{}", found.join("\n"));
}

fn ids_outside_fences(text: &str) -> Vec<&str> {
    prose_lines(text).into_iter().filter_map(|(_, line)| rule_id(line)).collect()
}

#[test]
fn a_longer_fence_can_hold_a_shorter_one_and_the_text_after_it_is_still_read() {
    let text = "- **X1.** a
````
```
- **X2.** inside
````
- **X3.** after
";
    assert_eq!(ids_outside_fences(text), ["X1", "X3"]);
}

#[test]
fn a_tilde_fence_hides_what_it_holds_and_the_text_after_it_is_still_read() {
    let text = "- **X1.** a
~~~
- **X2.** inside
```
~~~
- **X1.** after
";
    assert_eq!(ids_outside_fences(text), ["X1", "X1"]);
}

#[test]
fn a_fence_with_text_after_its_marks_does_not_close_a_fence() {
    let text = "```
```rust
- **X2.** inside
```
- **X3.** after
";
    assert_eq!(ids_outside_fences(text), ["X3"]);
}
