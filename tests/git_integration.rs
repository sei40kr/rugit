//! End-to-end tests of the git layer against a real `git` binary in a temp
//! repository: snapshot reading and hunk/line staging via generated patches.

use std::fs;
use std::path::Path;
use std::process::Command;

use rugit::git::client::GitClient;
use rugit::git::patch::{hunk_patch, line_patch, LineOp};

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
