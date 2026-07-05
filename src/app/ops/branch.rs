//! The branch transient: checkout via a picker, branch creation via the
//! minibuffer.

use crate::app::{svec, App};
use crate::ui::input::{InputPurpose, InputState};
use crate::ui::transient::TransientAction;

impl App {
    pub(super) fn branch_action(&mut self, action: TransientAction) {
        match action {
            TransientAction::Checkout => {
                self.input = Some(InputState::picker(
                    "Checkout",
                    InputPurpose::CheckoutRev,
                    self.list_branches(),
                ));
            }
            TransientAction::CreateCheckoutBranch => {
                self.input = Some(InputState::plain(
                    "Create and checkout branch",
                    InputPurpose::CreateCheckoutBranch,
                ));
            }
            TransientAction::CreateBranch => {
                self.input = Some(InputState::plain(
                    "Create branch",
                    InputPurpose::CreateBranch,
                ));
            }
            _ => unreachable!("not a branch action"),
        }
    }

    pub(super) fn branch_submit(&mut self, purpose: InputPurpose, value: String) {
        match purpose {
            // `git checkout` DWIMs: local branch, remote-tracking branch
            // (creates a local branch), tag or raw revision (detaches).
            InputPurpose::CheckoutRev => {
                self.run_git_bg(
                    format!("checkout {value}"),
                    svec(&["checkout", &value]),
                    None,
                );
            }
            InputPurpose::CreateCheckoutBranch => {
                self.run_git_bg(
                    format!("create+checkout {value}"),
                    svec(&["checkout", "-b", &value]),
                    None,
                );
            }
            InputPurpose::CreateBranch => {
                self.run_git_bg(
                    format!("create branch {value}"),
                    svec(&["branch", &value]),
                    None,
                );
            }
            _ => unreachable!("not a branch input"),
        }
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
