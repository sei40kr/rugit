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
    self, menu_def, Item, TransientAction, TransientDef, TransientState, CHERRY_PICK_IN_PROGRESS,
    MERGE_IN_PROGRESS, REBASE_IN_PROGRESS, REVERT_IN_PROGRESS,
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
        let mut st = TransientState::new(def);
        st.scope = self.transient_scope(menu);
        self.load_transient_variables(&mut st);
        self.transient = Some(st);
    }

    /// What `{}` in the menu's variable items resolves to. Menus without
    /// variables have no scope.
    fn transient_scope(&self, menu: Menu) -> Option<String> {
        match menu {
            Menu::Remote => self.current_remote(),
            Menu::Branch | Menu::Pull => self.snapshot.as_ref().and_then(|s| s.branch.head.clone()),
            _ => None,
        }
    }

    /// Read the current values of every variable item in the definition.
    fn load_transient_variables(&self, st: &mut TransientState) {
        let scope = st.scope.clone().unwrap_or_default();
        for group in st.def.groups {
            for item in group.items {
                if let Item::Variable { var, .. } = item {
                    if var.contains("{}") && st.scope.is_none() {
                        continue;
                    }
                    st.set_variable(var, self.read_transient_variable(var, &scope));
                    self.load_transient_var_choices(st, var);
                }
            }
        }
    }

    /// For a choice-cycling variable, record its choices and the
    /// trailing fallback/default segment.
    fn load_transient_var_choices(&self, st: &mut TransientState, var: &'static str) {
        let to_vec = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // (choices, fallback variable, built-in default) per variable.
        let (choices, fallback, default): (Vec<String>, Option<&str>, Option<&str>) = match var {
            "branch.{}.rebase" => (
                to_vec(&["true", "false"]),
                Some("pull.rebase"),
                Some("false"),
            ),
            "pull.rebase" => (to_vec(&["true", "false"]), None, Some("false")),
            "remote.{}.tagOpt" => (to_vec(&["--no-tags", "--tags"]), None, None),
            "remote.{}.followRemoteHEAD" => {
                (to_vec(&["create", "always", "warn"]), None, Some("create"))
            }
            // The push-remote choices are the actual remotes.
            "branch.{}.pushRemote" => (self.list_remotes(), Some("remote.pushDefault"), None),
            "remote.pushDefault" => (self.list_remotes(), None, None),
            _ => return,
        };
        // The fallback variable's value wins over the built-in default
        // ("pull.rebase:true" vs "default:false").
        let segment = fallback
            .and_then(|f| {
                self.read_transient_variable_raw(f)
                    .map(|v| format!("{f}:{v}"))
            })
            .or_else(|| default.map(|d| format!("default:{d}")));
        st.var_choices.insert(var, choices);
        if let Some(segment) = segment {
            st.var_fallbacks.insert(var, segment);
        }
    }

    /// `git config --get` of a literal (unscoped) variable name.
    fn read_transient_variable_raw(&self, name: &str) -> Option<String> {
        self.git
            .run(&["config", "--get", name])
            .ok()
            .filter(|o| o.status == 0)
            .map(|o| o.stdout.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// A variable's current value. The upstream pseudo-variable
    /// (`branch.{}.upstream`) reads the effective upstream instead of one
    /// config key, since it stands for `branch.<name>.merge`/`.remote`.
    fn read_transient_variable(&self, var: &'static str, scope: &str) -> Option<String> {
        let ask = |args: &[&str]| {
            self.git
                .run(args)
                .ok()
                .filter(|o| o.status == 0)
                .map(|o| o.stdout.trim().to_string())
                .filter(|s| !s.is_empty())
        };
        if var == "branch.{}.upstream" {
            ask(&[
                "rev-parse",
                "--abbrev-ref",
                &format!("{scope}@{{upstream}}"),
            ])
        } else {
            ask(&["config", "--get", &transient::resolve_var(var, scope)])
        }
    }

    /// Edit a variable: choice variables cycle to the next value in place
    /// (unset → first → ... → last → unset); the rest open a picker over
    /// "(unset)" plus the variable's known values, prefilled with the
    /// current value.
    pub(in crate::app) fn edit_transient_variable(&mut self, var: &'static str) {
        let Some(t) = self.transient.as_ref() else {
            return;
        };
        if let Some(choices) = t.var_choices.get(var) {
            let next = match t
                .variables
                .get(var)
                .and_then(|c| choices.iter().position(|x| x == c))
            {
                None => choices.first().cloned(),
                Some(i) if i + 1 < choices.len() => Some(choices[i + 1].clone()),
                Some(_) => None,
            };
            self.set_transient_variable(var, next.unwrap_or_default());
            return;
        }
        let scope = t.scope.clone().unwrap_or_default();
        let resolved = transient::resolve_var(var, &scope);
        let mut state = InputState::picker(
            format!("Set {resolved}"),
            self.transient_var_prompt_candidates(var),
        );
        if let Some(current) = t.variables.get(var) {
            state = state.with_text(current.clone());
        }
        // Unsetting goes through the "(unset)" entry (or clearing the
        // text); a picker cannot submit empty text directly.
        self.open_input_state(state, true, move |app, value| {
            let value = if value == "(unset)" {
                String::new()
            } else {
                value
            };
            app.set_transient_variable(var, value);
        });
    }

    /// Picker candidates for a variable's prompt: "(unset)" (standing in
    /// for clearing the value), plus the variable's known values.
    fn transient_var_prompt_candidates(&self, var: &'static str) -> Vec<String> {
        let mut out = vec!["(unset)".to_string()];
        // The upstream is one of the existing branches.
        if var == "branch.{}.upstream" {
            out.extend(self.list_branches());
        }
        out
    }

    /// Write a variable's new value to git config (empty unsets) and
    /// re-read it into the still-open transient.
    fn set_transient_variable(&mut self, var: &'static str, value: String) {
        let scope = self
            .transient
            .as_ref()
            .and_then(|t| t.scope.clone())
            .unwrap_or_default();
        let result = if var == "branch.{}.upstream" {
            // The pseudo-variable edits branch.<name>.merge/.remote in one
            // step, which git does consistently via --set-upstream-to.
            if value.is_empty() {
                self.git.run(&["branch", "--unset-upstream", &scope])
            } else {
                self.git
                    .run(&["branch", &format!("--set-upstream-to={value}"), &scope])
            }
        } else {
            let name = transient::resolve_var(var, &scope);
            if value.is_empty() {
                self.git.run(&["config", "--unset", &name])
            } else {
                self.git.run(&["config", &name, &value])
            }
        };
        // `config --unset` of a missing key exits 5: already unset is fine.
        let ok = match &result {
            Ok(out) => out.ok() || (value.is_empty() && out.status == 5),
            Err(_) => false,
        };
        if ok {
            let fresh = self.read_transient_variable(var, &scope);
            if let Some(t) = self.transient.as_mut() {
                t.set_variable(var, fresh);
            }
        } else {
            let detail = match result {
                Ok(out) => out.stderr.lines().next().unwrap_or_default().to_string(),
                Err(e) => e.to_string(),
            };
            self.message = Some(format!("setting variable failed: {detail}"));
        }
    }

    /// Open a variables-only transient (the branch/remote configure
    /// menus) for an explicitly chosen scope.
    pub(super) fn open_configure_transient(&mut self, def: &'static TransientDef, scope: String) {
        let mut st = TransientState::new(def);
        st.scope = Some(scope);
        self.load_transient_variables(&mut st);
        self.transient = Some(st);
    }

    /// Route a transient action to the module that owns its menu.
    pub(super) fn invoke_transient(&mut self, action: TransientAction, args: Vec<String>) {
        use TransientAction::*;
        match action {
            Commit | CommitAmend | CommitExtend => self.commit_action(action, args),
            PushToPushRemote | PushToUpstream | PushElsewhere | PushOther | PushRefspecs
            | PushMatching | PushTag | PushTags | PullFromPushRemote | PullFromUpstream
            | PullElsewhere | FetchFromPushRemote | FetchFromUpstream | FetchElsewhere
            | FetchAll | FetchBranch | FetchRefspec => self.remote_action(action, args),
            Checkout | CheckoutLocal | CreateCheckoutBranch | CreateBranch | BranchSpinoff
            | BranchSpinout | BranchRename | BranchReset | BranchDelete | BranchConfigure => {
                self.branch_action(action)
            }
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
            RemoteConfigure | RemoteAdd | RemoteRename | RemoteRemove | RemotePrune => {
                self.remote_menu_action(action, args)
            }
            LogCurrent | LogRelated | LogLocalBranches | LogAllBranches | LogAll | LogOther
            | ReflogCurrent | ReflogOther | ReflogHead => self.log_action(action, args),
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
            // Recent commit authors.
            "--author=" => self.list_recent_authors(),
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
