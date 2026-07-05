//! DWIM commands that act on the section under the cursor: stage, unstage,
//! discard, visit. `s` stages a file, a hunk, or a single diff line
//! depending on where point is.

use std::thread;

use crate::git::patch::{self, LineOp};
use crate::git::types::DiffArea;
use crate::ui::section::{Group, SectionValue};

use super::{svec, App, AppEvent, Confirm, PendingAction};

impl App {
    pub(super) fn stage_at_point(&mut self) {
        let Some(pane) = self.panes.last() else {
            return;
        };
        match pane.value_at_cursor() {
            SectionValue::File {
                area: DiffArea::Untracked | DiffArea::Unstaged,
                path,
            } => {
                self.run_git_bg(format!("stage {path}"), svec(&["add", "--", &path]), None);
            }
            SectionValue::Hunk {
                area: DiffArea::Unstaged,
                path,
                hunk_idx,
            } => {
                if let Some(patch) =
                    self.patch_at_point(DiffArea::Unstaged, &path, hunk_idx, LineOp::Stage)
                {
                    self.run_git_bg(
                        format!("stage hunk in {path}"),
                        svec(&["apply", "--cached", "--recount", "--whitespace=nowarn"]),
                        Some(patch),
                    );
                }
            }
            SectionValue::Group(Group::Untracked) => {
                let mut args = svec(&["add", "--"]);
                if let Some(s) = &self.snapshot {
                    args.extend(s.untracked.iter().cloned());
                }
                self.run_git_bg("stage untracked files".into(), args, None);
            }
            SectionValue::Group(Group::Unstaged) => {
                self.run_git_bg("stage all tracked".into(), svec(&["add", "-u"]), None);
            }
            SectionValue::File {
                area: DiffArea::Staged,
                ..
            }
            | SectionValue::Hunk {
                area: DiffArea::Staged,
                ..
            }
            | SectionValue::Group(Group::Staged) => {
                self.message = Some("already staged".into());
            }
            _ => self.message = Some("nothing to stage here".into()),
        }
    }

    pub(super) fn unstage_at_point(&mut self) {
        let Some(pane) = self.panes.last() else {
            return;
        };
        match pane.value_at_cursor() {
            SectionValue::File {
                area: DiffArea::Staged,
                path,
            } => {
                let args = if self.head_exists() {
                    svec(&["restore", "--staged", "--", &path])
                } else {
                    svec(&["rm", "--cached", "-r", "-q", "--", &path])
                };
                self.run_git_bg(format!("unstage {path}"), args, None);
            }
            SectionValue::Hunk {
                area: DiffArea::Staged,
                path,
                hunk_idx,
            } => {
                if let Some(patch) =
                    self.patch_at_point(DiffArea::Staged, &path, hunk_idx, LineOp::Unstage)
                {
                    self.run_git_bg(
                        format!("unstage hunk in {path}"),
                        svec(&[
                            "apply",
                            "-R",
                            "--cached",
                            "--recount",
                            "--whitespace=nowarn",
                        ]),
                        Some(patch),
                    );
                }
            }
            SectionValue::Group(Group::Staged) => self.unstage_all(),
            SectionValue::File {
                area: DiffArea::Untracked | DiffArea::Unstaged,
                ..
            }
            | SectionValue::Hunk {
                area: DiffArea::Unstaged,
                ..
            } => {
                self.message = Some("not staged".into());
            }
            _ => self.message = Some("nothing to unstage here".into()),
        }
    }

    pub(super) fn unstage_all(&mut self) {
        let args = if self.head_exists() {
            svec(&["restore", "--staged", "--", "."])
        } else {
            svec(&["rm", "--cached", "-r", "-q", "--", "."])
        };
        self.run_git_bg("unstage all".into(), args, None);
    }

    pub(super) fn discard_at_point(&mut self) {
        let Some(pane) = self.panes.last() else {
            return;
        };
        match pane.value_at_cursor() {
            SectionValue::File {
                area: DiffArea::Untracked,
                path,
            } => {
                self.confirm = Some(Confirm {
                    prompt: format!("Delete untracked {path}?"),
                    action: PendingAction::DeletePaths(vec![path]),
                });
            }
            SectionValue::File {
                area: DiffArea::Unstaged,
                path,
            } => {
                self.confirm = Some(Confirm {
                    prompt: format!("Discard changes to {path}?"),
                    action: PendingAction::Git {
                        desc: format!("discard {path}"),
                        args: svec(&["restore", "--", &path]),
                        stdin: None,
                    },
                });
            }
            SectionValue::Hunk {
                area: DiffArea::Unstaged,
                path,
                hunk_idx,
            } => {
                // Whole-hunk only: line-level discard is easy to fat-finger.
                let Some(pane) = self.panes.last() else {
                    return;
                };
                let Some(fd) = pane.find_file(DiffArea::Unstaged, &path) else {
                    return;
                };
                let Some(hunk) = fd.hunks.get(hunk_idx) else {
                    return;
                };
                let patch = patch::hunk_patch(fd, hunk);
                self.confirm = Some(Confirm {
                    prompt: format!("Discard this hunk in {path}?"),
                    action: PendingAction::Git {
                        desc: format!("discard hunk in {path}"),
                        args: svec(&["apply", "-R", "--recount", "--whitespace=nowarn"]),
                        stdin: Some(patch),
                    },
                });
            }
            SectionValue::Group(Group::Untracked) => {
                let paths: Vec<String> = self
                    .snapshot
                    .as_ref()
                    .map(|s| s.untracked.clone())
                    .unwrap_or_default();
                if paths.is_empty() {
                    self.message = Some("nothing to discard here".into());
                    return;
                }
                self.confirm = Some(Confirm {
                    prompt: format!("Delete {} untracked file(s)?", paths.len()),
                    action: PendingAction::DeletePaths(paths),
                });
            }
            SectionValue::Group(Group::Unstaged) => {
                self.confirm = Some(Confirm {
                    prompt: "Discard all unstaged changes?".into(),
                    action: PendingAction::Git {
                        desc: "discard all unstaged".into(),
                        args: svec(&["restore", "--", "."]),
                        stdin: None,
                    },
                });
            }
            SectionValue::Group(Group::Staged) => {
                // Restoring to HEAD wipes the index and worktree for the
                // staged paths; without a HEAD there is nothing to restore to.
                if !self.head_exists() {
                    self.message = Some("nothing committed yet".into());
                    return;
                }
                let paths = self.staged_paths();
                if paths.is_empty() {
                    self.message = Some("nothing to discard here".into());
                    return;
                }
                let mut args = svec(&["restore", "--staged", "--worktree", "--"]);
                args.extend(paths);
                self.confirm = Some(Confirm {
                    prompt: "Discard all staged changes?".into(),
                    action: PendingAction::Git {
                        desc: "discard all staged".into(),
                        args,
                        stdin: None,
                    },
                });
            }
            SectionValue::File {
                area: DiffArea::Staged,
                ..
            }
            | SectionValue::Hunk {
                area: DiffArea::Staged,
                ..
            } => {
                self.message = Some("unstage first, then discard".into());
            }
            _ => self.message = Some("nothing to discard here".into()),
        }
    }

    /// Build the patch for the hunk at point: single-line when the cursor is
    /// on a `+`/`-` body line, the whole hunk otherwise.
    fn patch_at_point(
        &self,
        area: DiffArea,
        path: &str,
        hunk_idx: usize,
        op: LineOp,
    ) -> Option<String> {
        let pane = self.panes.last()?;
        let fd = pane.find_file(area, path)?;
        let hunk = fd.hunks.get(hunk_idx)?;
        let cur = pane.current()?;
        if let Some(line_idx) = cur.body_idx.filter(|_| !cur.is_heading) {
            if let Some(p) = patch::line_patch(fd, hunk, line_idx, op) {
                return Some(p);
            }
        }
        Some(patch::hunk_patch(fd, hunk))
    }

    pub(super) fn visit_at_point(&mut self) {
        let Some(pane) = self.panes.last() else {
            return;
        };
        let rev = match pane.value_at_cursor() {
            SectionValue::Commit { hash } => hash,
            SectionValue::Stash { index } => format!("stash@{{{index}}}"),
            _ => {
                self.message = Some("nothing to visit here".into());
                return;
            }
        };
        self.busy = Some(format!("loading {rev}"));
        let git = self.git.clone();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let header = git
                .run(&["show", "--no-patch", "--format=medium", &rev])
                .map(|o| o.stdout)
                .unwrap_or_default();
            let diff = git
                .run(&["show", "--format=", "--patch", "--no-ext-diff", &rev])
                .map(|o| o.stdout)
                .unwrap_or_default();
            let _ = tx.send(AppEvent::RevisionReady {
                title: rev,
                header,
                diff,
            });
        });
    }

    fn head_exists(&self) -> bool {
        self.snapshot
            .as_ref()
            .map(|s| s.branch.oid.is_some())
            .unwrap_or(true)
    }

    /// All paths with staged changes, including the pre-rename path so a
    /// `restore` reverts renames cleanly.
    fn staged_paths(&self) -> Vec<String> {
        let Some(s) = &self.snapshot else {
            return Vec::new();
        };
        let mut paths = Vec::new();
        for fd in s.staged.iter() {
            paths.push(fd.path.clone());
            if let Some(old) = &fd.old_path {
                paths.push(old.clone());
            }
        }
        paths
    }
}
