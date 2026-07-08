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
use crate::ui::input::InputState;
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

    /// Prompt for a transient value-argument over the still-open transient;
    /// the continuation writes the value back into `transient.values` (empty
    /// clears the argument).
    pub(in crate::app) fn prompt_transient_arg(&mut self, flag: &'static str, desc: &'static str) {
        self.open_input_state(InputState::plain(desc), true, move |app, value| {
            if let Some(t) = app.transient.as_mut() {
                t.set_value(flag, value);
            }
        });
    }
}
