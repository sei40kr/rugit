//! The branch transient: checkout via a picker, branch creation via the
//! minibuffer.

use crate::app::{svec, App};
use crate::ui::section::SectionValue;
use crate::ui::transient::TransientAction;

impl App {
    pub(super) fn branch_action(&mut self, action: TransientAction) {
        match action {
            TransientAction::Checkout => {
                let revs = self.list_branches();
                self.open_picker("Checkout", revs, |app, rev| app.checkout_rev(rev));
            }
            TransientAction::CreateCheckoutBranch => {
                self.open_input("Create and checkout branch", |app, name| {
                    app.run_git_bg(
                        format!("create+checkout {name}"),
                        svec(&["checkout", "-b", &name]),
                        None,
                    );
                });
            }
            TransientAction::CreateBranch => {
                self.open_input("Create branch", |app, name| {
                    app.run_git_bg(
                        format!("create branch {name}"),
                        svec(&["branch", &name]),
                        None,
                    );
                });
            }
            _ => unreachable!("not a branch action"),
        }
    }

    /// `git checkout` DWIMs: local branch, remote-tracking branch (creates
    /// a local branch), tag or raw revision (detaches).
    fn checkout_rev(&mut self, rev: String) {
        self.run_git_bg(format!("checkout {rev}"), svec(&["checkout", &rev]), None);
    }

    /// `list_branches`, but with the commit at point (if any) as the first
    /// candidate — merge/rebase pickers default to the thing under the
    /// cursor, like Magit's at-point defaults.
    pub(super) fn list_revs_at_point(&self) -> Vec<String> {
        let mut out = self.list_branches();
        if let Some(SectionValue::Commit { hash }) = self.panes.last().map(|p| p.value_at_cursor())
        {
            out.retain(|b| *b != hash);
            out.insert(0, hash);
        }
        out
    }

    /// Local and remote-tracking branch names for the checkout picker.
    /// Listing refs is a fast local read, so this runs synchronously.
    pub(super) fn list_branches(&self) -> Vec<String> {
        let out = self
            .git
            .run(&["branch", "--all", "--format=%(refname:short)"])
            .ok();
        let mut seen = std::collections::BTreeSet::new();
        out.map(|o| o.stdout)
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.ends_with("HEAD") && !l.contains("HEAD detached"))
            .filter(|l| seen.insert(l.to_string()))
            .map(str::to_string)
            .collect()
    }
}
