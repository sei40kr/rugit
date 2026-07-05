//! One module per transient menu, mirroring how Magit splits into
//! magit-commit.el / magit-branch.el / magit-push.el. Adding a menu (stash,
//! bisect, ...) means: a `TransientDef` + `menu_def` arm in `ui/transient.rs`,
//! a `Menu` variant in `command.rs`, a new file here, and one routing arm in
//! each of the two matches below. `App::dispatch` never grows.

mod branch;
mod commit;
mod log;
mod merge;
mod rebase;
mod remote;

use crate::command::Menu;
use crate::ui::input::InputPurpose;
use crate::ui::transient::{
    menu_def, TransientAction, TransientState, MERGE_IN_PROGRESS, REBASE_IN_PROGRESS,
};

use super::App;

impl App {
    /// Open a transient menu. Most menus are the static `menu_def`; menus
    /// whose contents depend on repo state pick their definition here.
    pub(super) fn open_transient(&mut self, menu: Menu) {
        let def = match menu {
            Menu::Merge if self.merging() => &MERGE_IN_PROGRESS,
            Menu::Rebase if self.rebasing() => &REBASE_IN_PROGRESS,
            _ => menu_def(menu),
        };
        self.transient = Some(TransientState::new(def));
    }

    /// Route a transient action to the module that owns its menu.
    pub(super) fn invoke_transient(&mut self, action: TransientAction, args: Vec<String>) {
        use TransientAction::*;
        match action {
            Commit | CommitAmend | CommitExtend => self.commit_action(action, args),
            Push | PushSetUpstream | Pull | Fetch | FetchAll => self.remote_action(action, args),
            Checkout | CreateCheckoutBranch | CreateBranch => self.branch_action(action),
            Merge | MergeEdit | MergeNoCommit | MergeSquash | MergeAbsorb | MergePreview
            | MergeInto | MergeAbort => self.merge_action(action, args),
            RebaseUpstream | RebaseElsewhere | RebaseInteractive | RebaseContinue | RebaseSkip
            | RebaseEditTodo | RebaseAbort => self.rebase_action(action, args),
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
            InputPurpose::MergeRev
            | InputPurpose::MergeEditRev
            | InputPurpose::MergeNoCommitRev
            | InputPurpose::MergeSquashRev
            | InputPurpose::MergeAbsorbRev
            | InputPurpose::MergePreviewRev
            | InputPurpose::MergeIntoRev => self.merge_submit(purpose, value, carry),
            InputPurpose::RebaseOntoRev | InputPurpose::RebaseInteractiveRev => {
                self.rebase_submit(purpose, value, carry)
            }
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
