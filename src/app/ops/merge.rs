//! The merge transient: pick a revision to merge into HEAD; the transient
//! flags ride into the picker's continuation as captures. Absorb and
//! merge-into chain merge + branch cleanup on a worker; preview shows the
//! would-be merge via `merge-tree` without committing. Abort confirms first.

use std::thread;

use crate::app::{svec, App, AppEvent, Confirm, EditorRequest, PendingAction};
use crate::ui::transient::TransientAction;

impl App {
    /// Whether a merge is in progress, per the latest status snapshot.
    /// Decides both the merge menu variant and whether abort is allowed.
    pub(super) fn merging(&self) -> bool {
        self.snapshot.as_ref().and_then(|s| s.state.as_deref()) == Some("merging")
    }

    pub(super) fn merge_action(&mut self, action: TransientAction, args: Vec<String>) {
        if action == TransientAction::MergeAbort {
            // Aborting throws away any conflict resolutions in progress,
            // so gate on an actual merge (the in-progress menu is chosen
            // from the snapshot, which can lag) and confirm like discard.
            if !self.merging() {
                self.message = Some("no merge in progress".into());
                return;
            }
            self.confirm = Some(Confirm {
                prompt: "Abort the merge in progress?".into(),
                action: PendingAction::Git {
                    desc: "abort merge".into(),
                    args: svec(&["merge", "--abort"]),
                    stdin: None,
                },
            });
            return;
        }
        let revs = self.list_revs_at_point();
        self.open_picker("Merge", revs, move |app, rev| {
            app.merge_rev(action, rev, args)
        });
    }

    /// Run the picked merge variant against `rev` with the transient's flags.
    fn merge_rev(&mut self, action: TransientAction, rev: String, flags: Vec<String>) {
        match action {
            TransientAction::MergeAbsorb => {
                // Merge the branch, then delete it; the delete only runs
                // when the merge succeeded.
                let mut merge = svec(&["merge", "--no-edit"]);
                merge.extend(flags);
                merge.push(rev.clone());
                self.run_git_seq_bg(
                    format!("absorb {rev}"),
                    vec![merge, svec(&["branch", "-d", &rev])],
                );
                return;
            }
            TransientAction::MergeInto => {
                // Merge the current branch into another and delete the
                // former. The merge makes the current branch fully merged,
                // so `-d` is safe.
                let Some(current) = self.snapshot.as_ref().and_then(|s| s.branch.head.clone())
                else {
                    self.message = Some("cannot merge into: not on a branch".into());
                    return;
                };
                let mut merge = svec(&["merge", "--no-edit"]);
                merge.extend(flags);
                merge.push(current.clone());
                self.run_git_seq_bg(
                    format!("merge {current} into {rev}"),
                    vec![
                        svec(&["checkout", &rev]),
                        merge,
                        svec(&["branch", "-d", &current]),
                    ],
                );
                return;
            }
            TransientAction::MergePreview => {
                self.preview_merge(rev);
                return;
            }
            _ => {}
        }
        let base: &[&str] = match action {
            TransientAction::Merge => &["merge", "--no-edit"],
            TransientAction::MergeEdit => &["merge", "--edit"],
            // A fast-forward cannot be stopped by --no-commit alone, so force
            // a real merge; otherwise "don't commit" would silently move HEAD.
            TransientAction::MergeNoCommit => &["merge", "--no-commit", "--no-ff"],
            TransientAction::MergeSquash => &["merge", "--squash"],
            _ => unreachable!("not a merge action"),
        };
        let mut args = svec(base);
        args.extend(flags);
        args.push(rev.clone());
        let desc = format!("merge {rev}");
        if action == TransientAction::MergeEdit {
            self.editor_request = Some(EditorRequest::new(desc, args));
        } else {
            self.run_git_bg(desc, args, None);
        }
    }

    /// Show what merging `rev` into HEAD would change, without touching the
    /// worktree or index: `merge-tree` computes the merged tree in the
    /// object store, and the diff of HEAD against it opens as a read-only
    /// revision buffer. Conflicts show up as marker lines in the diff.
    fn preview_merge(&mut self, rev: String) {
        self.busy = Some(format!("previewing merge of {rev}"));
        let git = self.git.clone();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let tree = git.run(&["merge-tree", "--write-tree", "--no-messages", "HEAD", &rev]);
            // Exit 0 = clean merge, 1 = conflicts; both print the tree id
            // first. Anything without one is a real error.
            let (oid, conflicted, err) = match &tree {
                Ok(out) => (
                    out.stdout.lines().next().unwrap_or("").trim().to_string(),
                    out.status == 1,
                    out.stderr.trim().to_string(),
                ),
                Err(e) => (String::new(), false, e.to_string()),
            };
            let (header, diff) = if oid.is_empty() {
                (
                    format!("Cannot preview merging {rev} into HEAD:\n{err}"),
                    String::new(),
                )
            } else {
                let mut header =
                    format!("Preview of merging {rev} into HEAD — nothing has been committed.");
                if conflicted {
                    header.push_str("\nThis merge would have conflicts (markers shown below).");
                }
                let diff = git
                    .run(&["diff", "--no-ext-diff", "HEAD", &oid])
                    .map(|o| o.stdout)
                    .unwrap_or_default();
                (header, diff)
            };
            let _ = tx.send(AppEvent::RevisionReady {
                title: format!("merge preview: {rev}"),
                header,
                diff,
            });
        });
    }
}
