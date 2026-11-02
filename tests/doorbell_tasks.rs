//! Tests for how the doorbell runs an owner's task (OT6, OT7), against a tiny fake worker: a script
//! that reports what it was given. The rules are in docs/routines.md. A comment such as `// OT7`
//! above a test names the rule it covers.

use std::{
    fs,
    ops::Deref,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use agenthangout::doorbell::{pr_url, run_task, TaskOutcome};

// These tests mutate the process environment and create executable files. Keep setup and
// child execution together: another test must not fork while an executable is open for writing.
static PROCESS_TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Debug)]
struct WorkerFixture {
    path: PathBuf,
    directory: PathBuf,
}
impl Deref for WorkerFixture {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.path
    }
}
impl Drop for WorkerFixture {
    fn drop(&mut self) {
        // Only this fixture's file/directory, never the shared target root or a worktree.
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_dir(&self.directory);
    }
}
fn fixture_path(name: &str) -> WorkerFixture {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"));
    fs::create_dir_all(root).unwrap();
    let directory =
        root.join(format!("doorbell-task-{}-{}", std::process::id(), agenthangout::auth::new_id()));
    fs::create_dir(&directory).unwrap();
    let path =
        directory.join(if cfg!(windows) { format!("{name}.cmd") } else { format!("{name}.sh") });
    WorkerFixture { path, directory }
}

/// A worker is one program path, so on Windows it is a `.cmd` and elsewhere a shell script.
fn worker(name: &str, body: &[&str]) -> WorkerFixture {
    let fixture = fixture_path(name);
    let path = &fixture.path;
    let mut script = if cfg!(windows) { "@echo off\r\n" } else { "#!/bin/sh\n" }.to_string();
    for line in body {
        script.push_str(line);
        script.push_str(if cfg!(windows) { "\r\n" } else { "\n" });
    }
    fs::write(path, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fixture
}

/// The doorbell has its own token and address in its environment, as it does when it is run.
fn doorbell_environment() {
    std::env::set_var("AGENTHANGOUT_TOKEN", "ah_bot_the-doorbells-own-token");
    std::env::set_var("AGENTHANGOUT_URL", "https://example.test");
    std::env::set_var("PLANTED_SECRET", "hunter2-planted");
}

// OT7
#[tokio::test]
async fn the_worker_gets_the_task_on_stdin_and_never_the_token() {
    let _guard = PROCESS_TEST.lock().await;
    doorbell_environment();
    let report = if cfg!(windows) {
        worker(
            "report",
            &[
                "echo token=[%AGENTHANGOUT_TOKEN%]",
                "echo url=[%AGENTHANGOUT_URL%]",
                "echo id=[%TASK_ID%]",
                "echo secret=[%PLANTED_SECRET%]",
                "echo repo=[%AGENTHANGOUT_TASK_REPO%]",
                "findstr \"^\"",
                "echo https://github.com/a/b/pull/5",
            ],
        )
    } else {
        worker(
            "report",
            &[
                "echo \"token=[$AGENTHANGOUT_TOKEN]\"",
                "echo \"url=[$AGENTHANGOUT_URL]\"",
                "echo \"id=[$TASK_ID]\"",
                "echo \"secret=[$PLANTED_SECRET]\"",
                "echo \"repo=[$AGENTHANGOUT_TASK_REPO]\"",
                "cat",
                "echo",
                "echo https://github.com/a/b/pull/5",
            ],
        )
    };
    let outcome =
        run_task(&report, "room-12", "fix the typo\nthen stop", Duration::from_secs(60), "a/b")
            .await;
    let TaskOutcome::Done(output) = outcome else { panic!("{outcome:?}") };
    let lines: Vec<_> = output.lines().map(str::trim).collect();
    let expected = ["token=[]", "url=[]", "id=[room-12]", "secret=[]", "repo=[a/b]"];
    assert_eq!(lines[..5], expected, "{output}");
    assert_eq!(lines[5], "fix the typo", "{output}");
    assert_eq!(lines[6], "then stop", "{output}");
    assert!(
        !output.contains("hunter2-planted")
            && !output.contains("the-doorbells-own-token")
            && !output.contains("example.test")
    );
    assert_eq!(pr_url(&output, "a/b").as_deref(), Some("https://github.com/a/b/pull/5"));
}

// OT6
#[tokio::test]
async fn a_worker_that_fails_or_cant_start_is_a_failure() {
    let _guard = PROCESS_TEST.lock().await;
    let fails = worker("fails", &["exit 3"]);
    let outcome = run_task(&fails, "x", "t", Duration::from_secs(60), "a/b").await;
    assert!(matches!(outcome, TaskOutcome::Failed(_)), "{outcome:?}");

    let missing = fixture_path("no-such-worker");
    let outcome = run_task(&missing, "x", "t", Duration::from_secs(60), "a/b").await;
    assert!(matches!(outcome, TaskOutcome::Failed(_)), "{outcome:?}");
}

// OT6
#[tokio::test]
async fn a_worker_that_runs_too_long_is_killed() {
    let _guard = PROCESS_TEST.lock().await;
    let slow = if cfg!(windows) {
        worker("slow", &["ping -n 60 127.0.0.1 >nul"])
    } else {
        worker("slow", &["sleep 60"])
    };
    let started = Instant::now();
    let outcome = run_task(&slow, "x", "t", Duration::from_secs(1), "a/b").await;
    assert_eq!(outcome, TaskOutcome::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(30), "{:?}", started.elapsed());
}

// A task worker must never automatically delete other, active, or unpublished worktrees.
#[test]
fn the_worker_preserves_worktrees_for_explicit_owner_cleanup() {
    let script = include_str!("../hooks/task-worker.ps1");
    assert!(!script.contains("Remove-Item"));
    assert!(!script.contains("worktree remove"));
    assert!(!script.contains("branch -D"));
}

#[tokio::test]
async fn worker_fixture_names_do_not_reuse_or_overwrite_an_existing_executable() {
    let _guard = PROCESS_TEST.lock().await;
    let first = worker("same-name", &["echo first"]);
    let second = worker("same-name", &["echo second"]);
    assert_ne!(first.path, second.path, "each fixture invocation needs its own executable path");
    assert!(fs::read_to_string(&*first).unwrap().contains("echo first"));
    assert!(fs::read_to_string(&*second).unwrap().contains("echo second"));
}
