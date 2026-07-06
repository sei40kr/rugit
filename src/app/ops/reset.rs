//! The reset transient: pick a revision, then move HEAD/index/worktree to
//! it with one of the mixed/soft/hard/keep/index/worktree
//! variants. The two variants that discard uncommitted changes (hard and
//! worktree) confirm after the revision is picked, like discard.

use crate::app::{svec, App, Confirm, PendingAction};
use crate::ui::transient::TransientAction;

impl App {
    pub(super) fn reset_action(&mut self, action: TransientAction) {
        let prompt = match action {
            TransientAction::ResetMixed => "Reset (mixed) to",
            TransientAction::ResetSoft => "Reset (soft) to",
            TransientAction::ResetHard => "Reset (hard) to",
            TransientAction::ResetKeep => "Reset (keep) to",
            TransientAction::ResetIndex => "Reset index to",
            TransientAction::ResetWorktree => "Reset worktree to",
            _ => unreachable!("not a reset action"),
        };
        let revs = self.list_revs_at_point();
        self.open_picker(prompt, revs, move |app, rev| app.reset_to(action, rev));
    }

    fn reset_to(&mut self, action: TransientAction, rev: String) {
        let (desc, args) = match action {
            TransientAction::ResetMixed => {
                (format!("reset to {rev}"), svec(&["reset", "--mixed", &rev]))
            }
            TransientAction::ResetSoft => (
                format!("soft reset to {rev}"),
                svec(&["reset", "--soft", &rev]),
            ),
            TransientAction::ResetKeep => (
                format!("keep reset to {rev}"),
                svec(&["reset", "--keep", &rev]),
            ),
            // The pathspec form resets index entries without moving HEAD;
            // `:/` covers the whole worktree wherever git runs.
            TransientAction::ResetIndex => (
                format!("reset index to {rev}"),
                svec(&["reset", &rev, "--", ":/"]),
            ),
            TransientAction::ResetHard => {
                self.confirm = Some(Confirm {
                    prompt: format!("Discard uncommitted changes and hard-reset to {rev}?"),
                    action: PendingAction::Git {
                        desc: format!("hard reset to {rev}"),
                        args: svec(&["reset", "--hard", &rev]),
                        stdin: None,
                    },
                });
                return;
            }
            // `restore --worktree` rewrites files to the revision without
            // touching HEAD or the index; tracked files that don't
            // exist in the target are left alone.
            TransientAction::ResetWorktree => {
                self.confirm = Some(Confirm {
                    prompt: format!("Discard worktree changes and reset files to {rev}?"),
                    action: PendingAction::Git {
                        desc: format!("reset worktree to {rev}"),
                        args: svec(&["restore", "--source", &rev, "--worktree", "--", ":/"]),
                        stdin: None,
                    },
                });
                return;
            }
            _ => unreachable!("not a reset action"),
        };
        self.run_git_bg(desc, args, None);
    }
}
