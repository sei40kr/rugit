//! One module per transient menu, mirroring how Magit splits into
//! magit-commit.el / magit-branch.el / magit-push.el. Adding a menu (stash,
//! bisect, ...) means: a `TransientDef` + `menu_def` arm in `ui/transient.rs`,
//! a `Menu` variant in `command.rs`, a new file here, and one routing arm in
//! each of the two matches below. `App::dispatch` never grows.

mod branch;
mod cherry_pick;
mod commit;
mod log;
mod merge;
mod rebase;
mod remote;
mod reset;
mod revert;
mod stash;
mod tag;

use crate::command::Menu;
use crate::ui::input::InputState;
use crate::ui::transient::{
    menu_def, TransientAction, TransientState, CHERRY_PICK_IN_PROGRESS, MERGE_IN_PROGRESS,
    REBASE_IN_PROGRESS, REVERT_IN_PROGRESS,
};

use super::App;

impl App {
    /// Open a transient menu. Most menus are the static `menu_def`; menus
    /// whose contents depend on repo state pick their definition here.
    pub(super) fn open_transient(&mut self, menu: Menu) {
        let def = match menu {
            Menu::Merge if self.merging() => &MERGE_IN_PROGRESS,
            Menu::Rebase if self.rebasing() => &REBASE_IN_PROGRESS,
            Menu::CherryPick if self.cherry_picking() => &CHERRY_PICK_IN_PROGRESS,
            Menu::Revert if self.reverting() => &REVERT_IN_PROGRESS,
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
            CherryPick | CherryPickApply | CherryPickContinue | CherryPickSkip
            | CherryPickAbort => self.cherry_pick_action(action, args),
            Revert | RevertNoCommit | RevertContinue | RevertSkip | RevertAbort => {
                self.revert_action(action, args)
            }
            ResetMixed | ResetSoft | ResetHard | ResetKeep | ResetIndex | ResetWorktree => {
                self.reset_action(action)
            }
            StashBoth | StashIndex | StashKeepIndex | StashApply | StashPop | StashDrop => {
                self.stash_action(action, args)
            }
            TagCreate | TagDelete => self.tag_action(action, args),
            LogCurrent | LogAll | LogOther => self.log_action(action, args),
        }
    }

    /// Prompt for a transient value-argument over the still-open transient;
    /// the continuation writes the value back into `transient.values` (empty
    /// clears the argument).
    pub(in crate::app) fn prompt_transient_arg(&mut self, flag: &'static str, desc: &'static str) {
        let candidates = self.transient_arg_candidates(flag);
        self.open_input_state(
            InputState::picker(desc, candidates),
            true,
            move |app, value| {
                // Signing-key candidates read "KEYID uid"; only the key id
                // goes into the flag.
                let value = if matches!(flag, "--local-user=" | "--gpg-sign=") {
                    value
                        .split_whitespace()
                        .next()
                        .unwrap_or_default()
                        .to_string()
                } else {
                    value
                };
                if let Some(t) = app.transient.as_mut() {
                    t.set_value(flag, value);
                }
            },
        );
    }

    /// Candidates for a transient value-argument's prompt. Free-text
    /// arguments get an empty list — the picker accepts typed text either
    /// way.
    fn transient_arg_candidates(&self, flag: &str) -> Vec<String> {
        let list = |items: &[&str]| items.iter().map(|s| s.to_string()).collect();
        match flag {
            // The merge strategies git ships with.
            "--strategy=" => list(&["resolve", "recursive", "octopus", "ours", "subtree"]),
            // Merge parents to replay relative to — almost always one of
            // the two sides; typed input covers octopus merges.
            "--mainline=" => list(&["1", "2"]),
            // Secret keys gpg can sign with.
            "--local-user=" | "--gpg-sign=" => self.list_gpg_signing_keys(),
            _ => Vec::new(),
        }
    }

    /// Secret GPG keys as "KEYID uid" picker candidates. Missing gpg (or
    /// no keys) leaves the list empty; the picker still accepts a typed
    /// key id.
    fn list_gpg_signing_keys(&self) -> Vec<String> {
        let Ok(out) = std::process::Command::new("gpg")
            .args(["--list-secret-keys", "--with-colons"])
            .output()
        else {
            return Vec::new();
        };
        let stdout = String::from_utf8_lossy(&out.stdout);
        let mut keys = Vec::new();
        let mut pending: Option<String> = None;
        for line in stdout.lines() {
            let fields: Vec<&str> = line.split(':').collect();
            match fields.first().copied() {
                Some("sec") => {
                    if let Some(id) = pending.take() {
                        keys.push(id);
                    }
                    pending = fields.get(4).map(|id| id.to_string());
                }
                Some("uid") => {
                    if let (Some(id), Some(uid)) = (pending.take(), fields.get(9)) {
                        keys.push(format!("{id} {uid}"));
                    }
                }
                _ => {}
            }
        }
        if let Some(id) = pending {
            keys.push(id);
        }
        keys
    }
}
