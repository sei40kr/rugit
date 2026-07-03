//! Shelling out to the `git` CLI. All reads use `--no-optional-locks` so that
//! refreshes never write `.git/index` (which would re-trigger the fs watcher).

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::parse;
use super::types::StatusSnapshot;

/// SHA-1 of the empty tree; lets `diff --cached` work on an unborn branch.
pub const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("failed to run git: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("git {cmd} failed ({code}): {stderr}")]
    Failed {
        cmd: String,
        code: i32,
        stderr: String,
    },
}

#[derive(Debug, Clone)]
pub struct GitOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl GitOutput {
    pub fn ok(&self) -> bool {
        self.status == 0
    }
}

/// One executed git command, kept for the `$` process-log buffer.
#[derive(Debug, Clone)]
pub struct ProcessEntry {
    pub cmd: String,
    pub status: i32,
    pub output: String,
}

#[derive(Debug, Clone)]
pub struct GitClient {
    pub repo_root: PathBuf,
    pub git_dir: PathBuf,
}

impl GitClient {
    /// Discover the repository containing `cwd`.
    pub fn discover(cwd: &Path) -> Result<Self, GitError> {
        let root = run_in(cwd, &["rev-parse", "--show-toplevel"], None)?;
        if !root.ok() {
            return Err(GitError::Failed {
                cmd: "rev-parse --show-toplevel".into(),
                code: root.status,
                stderr: root.stderr.trim().to_string(),
            });
        }
        let repo_root = PathBuf::from(root.stdout.trim_end());
        let gd = run_in(&repo_root, &["rev-parse", "--absolute-git-dir"], None)?;
        let git_dir = PathBuf::from(gd.stdout.trim_end());
        Ok(Self { repo_root, git_dir })
    }

    /// Run git; non-zero exit is reported via `GitOutput::status`, not `Err`.
    pub fn run(&self, args: &[&str]) -> Result<GitOutput, GitError> {
        run_in(&self.repo_root, args, None)
    }

    pub fn run_with_input(&self, args: &[&str], stdin: &str) -> Result<GitOutput, GitError> {
        run_in(&self.repo_root, args, Some(stdin))
    }

    /// Like `run` but turns a non-zero exit into `Err` — for reads that must succeed.
    fn read(&self, args: &[&str]) -> Result<GitOutput, GitError> {
        let out = self.run(args)?;
        if out.ok() {
            Ok(out)
        } else {
            Err(GitError::Failed {
                cmd: args.join(" "),
                code: out.status,
                stderr: out.stderr.trim().to_string(),
            })
        }
    }

    /// Read everything the status buffer needs. Runs on a worker thread.
    pub fn read_snapshot(&self) -> Result<StatusSnapshot, GitError> {
        let status = self.read(&["status", "--porcelain=v2", "--branch", "-z"])?;
        let st = parse::parse_status_v2(&status.stdout);

        let unstaged = parse::parse_diff(&self.read(&["diff", "--no-ext-diff"])?.stdout);
        let staged_args: &[&str] = if st.branch.oid.is_some() {
            &["diff", "--no-ext-diff", "--cached"]
        } else {
            &["diff", "--no-ext-diff", "--cached", EMPTY_TREE]
        };
        let staged = parse::parse_diff(&self.read(staged_args)?.stdout);

        let (head_summary, recent) = if st.branch.oid.is_some() {
            let summary = self.read(&["log", "-1", "--format=%h %s"])?;
            let log = self.read(&["log", "-n", "10", "--format=%h\u{1f}%s"])?;
            (
                Some(summary.stdout.trim_end().to_string()),
                parse::parse_log(&log.stdout),
            )
        } else {
            (None, Vec::new())
        };

        let stashes = self.run(&["stash", "list", "--format=%gd\u{1f}%s"])?.stdout;

        Ok(StatusSnapshot {
            branch: st.branch,
            head_summary,
            state: self.repo_state(),
            untracked: st.untracked,
            unmerged: st.unmerged,
            unstaged,
            staged,
            stashes: parse::parse_stash_list(&stashes),
            recent,
        })
    }

    /// In-progress operation, detected from marker files in the git dir.
    fn repo_state(&self) -> Option<String> {
        let gd = &self.git_dir;
        if gd.join("rebase-merge").exists() || gd.join("rebase-apply").exists() {
            Some("rebasing".into())
        } else if gd.join("MERGE_HEAD").exists() {
            Some("merging".into())
        } else if gd.join("CHERRY_PICK_HEAD").exists() {
            Some("cherry-picking".into())
        } else if gd.join("REVERT_HEAD").exists() {
            Some("reverting".into())
        } else if gd.join("BISECT_LOG").exists() {
            Some("bisecting".into())
        } else {
            None
        }
    }
}

fn run_in(dir: &Path, args: &[&str], stdin: Option<&str>) -> Result<GitOutput, GitError> {
    let mut cmd = Command::new("git");
    cmd.arg("--no-pager")
        .arg("--no-optional-locks")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    if let Some(input) = stdin {
        // The child may exit without draining stdin; a write error then is fine.
        if let Some(mut pipe) = child.stdin.take() {
            let _ = pipe.write_all(input.as_bytes());
        }
    }
    let out = child.wait_with_output()?;
    Ok(GitOutput {
        status: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

/// Human-readable command line for messages and the process log.
pub fn display_cmd(args: &[String]) -> String {
    let mut s = String::from("git");
    for a in args {
        s.push(' ');
        if a.contains(' ') {
            s.push('\'');
            s.push_str(a);
            s.push('\'');
        } else {
            s.push_str(a);
        }
    }
    s
}
