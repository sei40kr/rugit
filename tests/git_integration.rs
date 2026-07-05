//! End-to-end tests of the git layer against a real `git` binary in a temp
//! repository: snapshot reading and hunk/line staging via generated patches.

use std::fs;
use std::path::Path;
use std::process::Command;

use rugit::git::client::GitClient;
use rugit::git::patch::{hunk_patch, line_patch, LineOp};
use rugit::git::todo::{self, TodoAction, TodoEntry};

fn sh(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("spawn git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn init_repo(dir: &Path) -> GitClient {
    sh(dir, &["init", "-q", "-b", "main"]);
    sh(dir, &["config", "user.name", "test"]);
    sh(dir, &["config", "user.email", "test@example.com"]);
    sh(dir, &["config", "commit.gpgsign", "false"]);
    GitClient::discover(dir).expect("discover repo")
}

#[test]
fn snapshot_of_fresh_repo_with_staged_file() {
    let tmp = tempfile::tempdir().unwrap();
    let git = init_repo(tmp.path());
    fs::write(tmp.path().join("a.txt"), "hello\n").unwrap();
    sh(tmp.path(), &["add", "a.txt"]);

    // Unborn branch: `diff --cached` must fall back to the empty tree.
    let snap = git.read_snapshot().unwrap();
    assert!(snap.branch.oid.is_none());
    assert_eq!(snap.staged.len(), 1);
    assert!(snap.staged[0].is_new);
    assert_eq!(snap.staged[0].path, "a.txt");
    assert!(snap.recent.is_empty());
}

#[test]
fn snapshot_untracked_and_unstaged() {
    let tmp = tempfile::tempdir().unwrap();
    let git = init_repo(tmp.path());
    fs::write(tmp.path().join("a.txt"), "one\ntwo\nthree\n").unwrap();
    sh(tmp.path(), &["add", "a.txt"]);
    sh(tmp.path(), &["commit", "-q", "-m", "init"]);

    fs::write(tmp.path().join("a.txt"), "one\nTWO\nthree\n").unwrap();
    fs::write(tmp.path().join("new.txt"), "x\n").unwrap();

    let snap = git.read_snapshot().unwrap();
    assert_eq!(snap.branch.head.as_deref(), Some("main"));
    assert_eq!(snap.untracked, vec!["new.txt"]);
    assert_eq!(snap.unstaged.len(), 1);
    assert_eq!(snap.unstaged[0].hunks.len(), 1);
    assert_eq!(snap.recent.len(), 1);
    assert!(snap.staged.is_empty());
}

#[test]
fn stage_and_unstage_whole_hunk_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let git = init_repo(tmp.path());
    fs::write(tmp.path().join("a.txt"), "one\ntwo\nthree\n").unwrap();
    sh(tmp.path(), &["add", "."]);
    sh(tmp.path(), &["commit", "-q", "-m", "init"]);
    fs::write(tmp.path().join("a.txt"), "one\nTWO\nthree\n").unwrap();

    let snap = git.read_snapshot().unwrap();
    let fd = &snap.unstaged[0];
    let patch = hunk_patch(fd, &fd.hunks[0]);
    let out = git
        .run_with_input(
            &["apply", "--cached", "--recount", "--whitespace=nowarn"],
            &patch,
        )
        .unwrap();
    assert!(out.ok(), "apply failed: {}", out.stderr);

    let snap = git.read_snapshot().unwrap();
    assert!(snap.unstaged.is_empty());
    assert_eq!(snap.staged.len(), 1);

    // Reverse-apply the staged hunk to unstage it again.
    let fd = &snap.staged[0];
    let patch = hunk_patch(fd, &fd.hunks[0]);
    let out = git
        .run_with_input(
            &[
                "apply",
                "-R",
                "--cached",
                "--recount",
                "--whitespace=nowarn",
            ],
            &patch,
        )
        .unwrap();
    assert!(out.ok(), "reverse apply failed: {}", out.stderr);
    let snap = git.read_snapshot().unwrap();
    assert!(snap.staged.is_empty());
    assert_eq!(snap.unstaged.len(), 1);
}

#[test]
fn conflicted_rebase_is_reported_as_rebasing() {
    let tmp = tempfile::tempdir().unwrap();
    let git = init_repo(tmp.path());
    fs::write(tmp.path().join("a.txt"), "base\n").unwrap();
    sh(tmp.path(), &["add", "a.txt"]);
    sh(tmp.path(), &["commit", "-q", "-m", "base"]);

    // Divergent edits of the same line on two branches.
    sh(tmp.path(), &["checkout", "-q", "-b", "topic"]);
    fs::write(tmp.path().join("a.txt"), "topic\n").unwrap();
    sh(tmp.path(), &["commit", "-q", "-am", "topic change"]);
    sh(tmp.path(), &["checkout", "-q", "main"]);
    fs::write(tmp.path().join("a.txt"), "main\n").unwrap();
    sh(tmp.path(), &["commit", "-q", "-am", "main change"]);
    sh(tmp.path(), &["checkout", "-q", "topic"]);

    // The rebase stops on the conflict (non-zero exit, so don't use `sh`).
    let out = git.run(&["rebase", "main"]).unwrap();
    assert!(!out.ok(), "rebase unexpectedly succeeded");

    let snap = git.read_snapshot().unwrap();
    assert_eq!(snap.state.as_deref(), Some("rebasing"));
    assert_eq!(snap.unmerged, vec!["a.txt"]);

    let out = git.run(&["rebase", "--abort"]).unwrap();
    assert!(out.ok(), "rebase --abort failed: {}", out.stderr);
    let snap = git.read_snapshot().unwrap();
    assert_eq!(snap.state, None);
}

/// The pipeline behind the in-app todo editor's confirm: seed picks from
/// `git log --reverse`, autosquash, serialize, and let git's sequencer run
/// the plan through `GIT_SEQUENCE_EDITOR` (a `cp` of the prepared file).
#[test]
fn prepared_todo_drives_interactive_rebase_via_sequence_editor() {
    let tmp = tempfile::tempdir().unwrap();
    let git = init_repo(tmp.path());
    for (file, content, msg) in [
        ("base.txt", "base\n", "base"),
        ("a.txt", "one\n", "add a"),
        ("b.txt", "two\n", "add b"),
        ("a.txt", "one fixed\n", "fixup! add a"),
    ] {
        fs::write(tmp.path().join(file), content).unwrap();
        sh(tmp.path(), &["add", "."]);
        sh(tmp.path(), &["commit", "-q", "-m", msg]);
    }

    // Seed the plan the way `open_todo_editor` does, oldest first.
    let log = git
        .run(&["log", "--reverse", "--format=%h\u{1f}%s", "HEAD~3..HEAD"])
        .unwrap();
    let entries: Vec<TodoEntry> = log
        .stdout
        .lines()
        .map(|l| {
            let (hash, subject) = l.split_once('\u{1f}').unwrap();
            TodoEntry {
                action: TodoAction::Pick,
                hash: hash.into(),
                subject: subject.into(),
            }
        })
        .collect();
    let plan = todo::serialize_todo(&todo::autosquash(entries));
    let plan_path = tmp.path().join("plan");
    fs::write(&plan_path, plan).unwrap();

    let status = Command::new("git")
        .args(["rebase", "--interactive", "HEAD~3"])
        .env(
            "GIT_SEQUENCE_EDITOR",
            format!("cp '{}'", plan_path.display()),
        )
        .env("GIT_EDITOR", "true")
        .env("GIT_TERMINAL_PROMPT", "0")
        .current_dir(tmp.path())
        .status()
        .expect("spawn git rebase");
    assert!(status.success(), "interactive rebase failed");

    // The fixup melted into "add a": three commits became two, the fix kept.
    let snap = git.read_snapshot().unwrap();
    assert_eq!(snap.state, None);
    let subjects: Vec<&str> = snap.recent.iter().map(|c| c.subject.as_str()).collect();
    assert_eq!(subjects, vec!["add b", "add a", "base"]);
    assert_eq!(
        fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
        "one fixed\n"
    );
}

#[test]
fn stage_single_line_from_hunk() {
    let tmp = tempfile::tempdir().unwrap();
    let git = init_repo(tmp.path());
    fs::write(tmp.path().join("a.txt"), "one\ntwo\n").unwrap();
    sh(tmp.path(), &["add", "."]);
    sh(tmp.path(), &["commit", "-q", "-m", "init"]);
    // Two added lines in one hunk; stage only the first.
    fs::write(tmp.path().join("a.txt"), "one\nalpha\nbeta\ntwo\n").unwrap();

    let snap = git.read_snapshot().unwrap();
    let fd = &snap.unstaged[0];
    let hunk = &fd.hunks[0];
    let alpha_idx = hunk
        .lines
        .iter()
        .position(|l| l == "+alpha")
        .expect("+alpha line present");
    let patch = line_patch(fd, hunk, alpha_idx, LineOp::Stage).unwrap();
    let out = git
        .run_with_input(
            &["apply", "--cached", "--recount", "--whitespace=nowarn"],
            &patch,
        )
        .unwrap();
    assert!(out.ok(), "apply failed: {}\npatch:\n{patch}", out.stderr);

    let snap = git.read_snapshot().unwrap();
    // "alpha" staged; "beta" still unstaged.
    let staged_lines: Vec<_> = snap.staged[0].hunks[0].lines.clone();
    assert!(staged_lines.contains(&"+alpha".to_string()));
    assert!(!staged_lines.iter().any(|l| l == "+beta"));
    let unstaged_lines: Vec<_> = snap.unstaged[0].hunks[0].lines.clone();
    assert!(unstaged_lines.contains(&"+beta".to_string()));
}
