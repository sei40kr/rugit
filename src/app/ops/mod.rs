//! One module per transient menu, mirroring how Magit splits into
//! magit-commit.el / magit-branch.el / magit-push.el. Adding a menu (merge,
//! rebase, stash, ...) means: a `TransientDef` in `ui/transient.rs`, a
//! `Command::TransientX` entry, a new file here, and one routing arm in each
//! of the two matches below. Nothing else grows.

mod branch;
mod commit;
mod log;
mod remote;

use crate::ui::input::InputPurpose;
use crate::ui::transient::TransientAction;

use super::App;

impl App {
    /// Route a transient action to the module that owns its menu.
    pub(super) fn invoke_transient(&mut self, action: TransientAction, args: Vec<String>) {
        use TransientAction::*;
        match action {
            Commit | CommitAmend | CommitExtend => self.commit_action(action, args),
            Push | PushSetUpstream | Pull | Fetch | FetchAll => self.remote_action(action, args),
            Checkout | CreateCheckoutBranch | CreateBranch => self.branch_action(action),
            LogCurrent | LogAll | LogOther => self.log_action(action, args),
        }
    }

    /// Route a submitted minibuffer value to the module that opened the input.
    pub(super) fn on_input_submit(
        &mut self,
        purpose: InputPurpose,
        value: String,
        carry: Vec<String>,
    ) {
        if purpose == InputPurpose::Search {
            self.search_submit(value);
            return;
        }
        if value.is_empty() {
            self.message = Some("empty input".into());
            return;
        }
        match purpose {
            InputPurpose::CheckoutRev
            | InputPurpose::CreateCheckoutBranch
            | InputPurpose::CreateBranch => self.branch_submit(purpose, value),
            InputPurpose::LogRev => self.log_submit(value, carry),
            InputPurpose::TransientArg(flag) => {
                if let Some(t) = self.transient.as_mut() {
                    t.set_value(flag, value);
                }
            }
            InputPurpose::Search => unreachable!("handled by the early return above"),
        }
    }
}
