//! Data-driven transient menus (Magit's popup system). A transient is pure
//! data: groups of switches and actions. While one is open it captures all
//! keys; actions collect the enabled switches into CLI flags.

use std::collections::{BTreeMap, BTreeSet};

use ratatui::crossterm::event::KeyCode;
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};

use crate::command::Menu;
use crate::keymap::KeyPress;
use crate::theme::Theme;

/// What an action ultimately runs. The app maps these to git invocations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransientAction {
    Commit,
    CommitAmend,
    CommitExtend,
    /// Push the current branch to its push-remote.
    PushToPushRemote,
    /// Push the current branch to its upstream.
    PushToUpstream,
    /// Opens a picker over remote branches, then pushes the current branch
    /// to the chosen target.
    PushElsewhere,
    /// Opens pickers for a local branch and its remote target, then pushes.
    PushOther,
    /// Opens a picker for the remote, then a minibuffer for the refspecs.
    PushRefspecs,
    /// Push all matching branches to a chosen remote.
    PushMatching,
    /// Opens pickers for a tag and the remote to push it to.
    PushTag,
    /// Push all tags to a chosen remote.
    PushTags,
    /// Pull into the current branch from its push-remote.
    PullFromPushRemote,
    /// Pull into the current branch from its upstream.
    PullFromUpstream,
    /// Opens a picker over remote branches, then pulls from the chosen one.
    PullElsewhere,
    /// Fetch from the current branch's push-remote.
    FetchFromPushRemote,
    /// Fetch from the current branch's upstream remote.
    FetchFromUpstream,
    /// Opens a picker over remotes, then fetches from the chosen one.
    FetchElsewhere,
    /// Fetch from all remotes.
    FetchAll,
    /// Opens a picker over remote branches, then fetches just that branch.
    FetchBranch,
    /// Opens a picker for the remote, then a minibuffer for the refspec.
    FetchRefspec,
    /// Opens a picker over branches, then merges headlessly (`--no-edit`).
    Merge,
    /// Like `Merge`, but hands the merge message to $EDITOR.
    MergeEdit,
    /// Merge without committing, so the result can be inspected first.
    MergeNoCommit,
    /// Squash the merged changes onto the worktree/index (no merge commit).
    MergeSquash,
    /// Merge a branch, then delete it (magit-merge-absorb).
    MergeAbsorb,
    /// Show what merging a revision would change, committing nothing.
    MergePreview,
    /// Check out another branch, merge the current one into it, then
    /// delete the current one (magit-merge-into).
    MergeInto,
    /// Abort the in-progress merge (after a y/n confirm).
    MergeAbort,
    /// Opens a picker over revisions, then cherry-picks onto HEAD.
    CherryPick,
    /// Cherry-pick without committing.
    CherryPickApply,
    /// Continue the in-progress cherry-pick (may open $EDITOR).
    CherryPickContinue,
    /// Skip the commit that stopped the in-progress cherry-pick.
    CherryPickSkip,
    /// Abort the in-progress cherry-pick (after a y/n confirm).
    CherryPickAbort,
    /// Opens a picker over revisions, then reverts with a new commit.
    Revert,
    /// Revert without committing.
    RevertNoCommit,
    /// Continue the in-progress revert (may open $EDITOR).
    RevertContinue,
    /// Skip the commit that stopped the in-progress revert.
    RevertSkip,
    /// Abort the in-progress revert (after a y/n confirm).
    RevertAbort,
    /// Rebase the current branch onto its upstream.
    RebaseUpstream,
    /// Opens a picker over branches, then rebases onto the chosen one.
    RebaseElsewhere,
    /// Like `RebaseElsewhere`, but interactive: the todo list opens in $EDITOR.
    RebaseInteractive,
    /// Continue the in-progress rebase (may open $EDITOR for a message).
    RebaseContinue,
    /// Skip the commit that stopped the in-progress rebase.
    RebaseSkip,
    /// Reopen the todo list of the in-progress interactive rebase in $EDITOR.
    RebaseEditTodo,
    /// Abort the in-progress rebase (after a y/n confirm).
    RebaseAbort,
    /// Reset HEAD and the index to a revision (`--mixed`).
    ResetMixed,
    /// Reset only HEAD to a revision (`--soft`).
    ResetSoft,
    /// Reset HEAD, index and worktree to a revision (`--hard`, confirmed).
    ResetHard,
    /// Reset HEAD and the index, keeping local changes (`--keep`).
    ResetKeep,
    /// Reset only the index to a revision (HEAD and worktree stay).
    ResetIndex,
    /// Reset only the worktree to a revision (HEAD and index stay,
    /// confirmed).
    ResetWorktree,
    /// Opens a minibuffer for the tag name, then a picker for the revision.
    TagCreate,
    /// Opens a picker over tags, then deletes the chosen one.
    TagDelete,
    /// Stash the worktree and index (`git stash push`).
    StashBoth,
    /// Stash only staged changes (`git stash push --staged`).
    StashIndex,
    /// Stash everything but leave the index applied (`--keep-index`).
    StashKeepIndex,
    /// Opens a picker over stashes, then applies the chosen one.
    StashApply,
    /// Like `StashApply`, but drops the stash afterwards.
    StashPop,
    /// Drop a stash (after a y/n confirm).
    StashDrop,
    /// Opens a picker over remotes, then a variables menu for the
    /// chosen one.
    RemoteConfigure,
    /// Opens a minibuffer for the remote name, then one for its URL.
    RemoteAdd,
    /// Opens a picker over remotes, then a minibuffer for the new name.
    RemoteRename,
    /// Opens a picker over remotes, then removes the chosen one.
    RemoteRemove,
    /// Opens a picker over remotes, then prunes stale tracking branches.
    RemotePrune,
    /// Opens a picker over branches and revisions, then checks it out.
    Checkout,
    /// Opens a picker over local branches only, then checks it out.
    CheckoutLocal,
    /// Opens a minibuffer for the new branch name, then a picker for the
    /// starting point, then checks the new branch out.
    CreateCheckoutBranch,
    /// Like `CreateCheckoutBranch`, but stays on the current branch.
    CreateBranch,
    /// Move the unpushed commits onto a new checked-out branch and reset
    /// the old branch to its upstream.
    BranchSpinoff,
    /// Like `BranchSpinoff`, but stay on the (reset) old branch.
    BranchSpinout,
    /// Opens a picker over local branches, then a minibuffer for its new name.
    BranchRename,
    /// Opens pickers for a local branch and the revision to reset it to.
    BranchReset,
    /// Opens a picker over local branches, then deletes the chosen one
    /// (confirmed first when it is unmerged).
    BranchDelete,
    /// Opens a picker over local branches, then a variables menu for
    /// the chosen one.
    BranchConfigure,
    /// Log the current branch (HEAD).
    LogCurrent,
    /// Log all refs (`--all`).
    LogAll,
    /// Opens a picker over refs, then logs the chosen one.
    LogOther,
}

#[derive(Debug, Clone, Copy)]
pub enum Item {
    /// A boolean flag, toggled on/off.
    Switch {
        key: &'static str,
        flag: &'static str,
        desc: &'static str,
    },
    /// A flag that takes a value (e.g. `--author=`). Selecting it prompts for
    /// the value; `flag` must end so the value appends directly (`--author=`,
    /// `--max-count=`). Set to empty to clear it.
    Arg {
        key: &'static str,
        flag: &'static str,
        desc: &'static str,
    },
    Action {
        key: &'static str,
        desc: &'static str,
        action: TransientAction,
    },
    /// A git config variable: shows the current value, and selecting it
    /// prompts for a new one (empty unsets). `{}` in
    /// `var` is replaced by the transient's scope (a branch or remote
    /// name); scoped variables are hidden while there is no scope.
    Variable {
        key: &'static str,
        var: &'static str,
    },
}

impl Item {
    fn key(&self) -> &'static str {
        match self {
            Item::Switch { key, .. }
            | Item::Arg { key, .. }
            | Item::Action { key, .. }
            | Item::Variable { key, .. } => key,
        }
    }
}

/// Resolve a variable template against the transient's scope:
/// `remote.{}.url` + "origin" → `remote.origin.url`.
pub fn resolve_var(var: &str, scope: &str) -> String {
    var.replace("{}", scope)
}

#[derive(Debug, Clone, Copy)]
pub struct GroupDef {
    pub title: &'static str,
    pub items: &'static [Item],
}

#[derive(Debug, Clone, Copy)]
pub struct TransientDef {
    pub title: &'static str,
    pub groups: &'static [GroupDef],
    /// Switch flags enabled when the menu opens (Magit's `:value`).
    pub defaults: &'static [&'static str],
    /// Sets of mutually exclusive switches (Magit's `:incompatible`):
    /// enabling one clears the others in its set.
    pub incompatible: &'static [&'static [&'static str]],
}

pub static COMMIT: TransientDef = TransientDef {
    title: "Commit",
    defaults: &[],
    incompatible: &[],
    groups: &[
        GroupDef {
            title: "Arguments",
            items: &[
                Item::Switch {
                    key: "-a",
                    flag: "--all",
                    desc: "Stage all modified and deleted files",
                },
                Item::Switch {
                    key: "-e",
                    flag: "--allow-empty",
                    desc: "Allow empty commit",
                },
                Item::Switch {
                    key: "-n",
                    flag: "--no-verify",
                    desc: "Disable hooks",
                },
                Item::Switch {
                    key: "-s",
                    flag: "--signoff",
                    desc: "Add Signed-off-by line",
                },
            ],
        },
        GroupDef {
            title: "Actions",
            items: &[
                Item::Action {
                    key: "c",
                    desc: "Commit",
                    action: TransientAction::Commit,
                },
                Item::Action {
                    key: "a",
                    desc: "Amend",
                    action: TransientAction::CommitAmend,
                },
                Item::Action {
                    key: "e",
                    desc: "Extend (amend, keep message)",
                    action: TransientAction::CommitExtend,
                },
            ],
        },
    ],
};

// Push the current branch to its push-remote/upstream or elsewhere, plus
// other branches, explicit refspecs, matching branches and tags.
pub static PUSH: TransientDef = TransientDef {
    title: "Push",
    defaults: &[],
    incompatible: &[],
    groups: &[
        GroupDef {
            title: "Arguments",
            items: &[
                Item::Switch {
                    key: "-f",
                    flag: "--force-with-lease",
                    desc: "Force with lease",
                },
                Item::Switch {
                    key: "-F",
                    flag: "--force",
                    desc: "Force",
                },
                Item::Switch {
                    key: "-h",
                    flag: "--no-verify",
                    desc: "Disable hooks",
                },
                Item::Switch {
                    key: "-n",
                    flag: "--dry-run",
                    desc: "Dry run",
                },
                Item::Switch {
                    key: "-T",
                    flag: "--tags",
                    desc: "Include all tags",
                },
                Item::Switch {
                    key: "-t",
                    flag: "--follow-tags",
                    desc: "Include related annotated tags",
                },
            ],
        },
        GroupDef {
            title: "Push current branch to",
            items: &[
                Item::Action {
                    key: "p",
                    desc: "push-remote",
                    action: TransientAction::PushToPushRemote,
                },
                Item::Action {
                    key: "u",
                    desc: "upstream",
                    action: TransientAction::PushToUpstream,
                },
                Item::Action {
                    key: "e",
                    desc: "elsewhere",
                    action: TransientAction::PushElsewhere,
                },
            ],
        },
        GroupDef {
            title: "Push",
            items: &[
                Item::Action {
                    key: "o",
                    desc: "another branch",
                    action: TransientAction::PushOther,
                },
                Item::Action {
                    key: "r",
                    desc: "explicit refspecs",
                    action: TransientAction::PushRefspecs,
                },
                Item::Action {
                    key: "m",
                    desc: "matching branches",
                    action: TransientAction::PushMatching,
                },
                Item::Action {
                    key: "T",
                    desc: "a tag",
                    action: TransientAction::PushTag,
                },
                Item::Action {
                    key: "t",
                    desc: "all tags",
                    action: TransientAction::PushTags,
                },
            ],
        },
        GroupDef {
            title: "Configure",
            items: &[Item::Action {
                key: "C",
                desc: "Set variables...",
                action: TransientAction::BranchConfigure,
            }],
        },
    ],
};

/// The per-branch variables, scoped to
/// the current branch in the branch menu and to a picked one in the
/// configure menu. `branch.{}.upstream` is a pseudo-variable standing for
/// `.merge`/`.remote` together.
const BRANCH_VARIABLES: GroupDef = GroupDef {
    title: "Configure branch",
    items: &[
        Item::Variable {
            key: "d",
            var: "branch.{}.description",
        },
        Item::Variable {
            key: "u",
            var: "branch.{}.upstream",
        },
        Item::Variable {
            key: "r",
            var: "branch.{}.rebase",
        },
        Item::Variable {
            key: "p",
            var: "branch.{}.pushRemote",
        },
    ],
};

// Checkout, creation (including spin-off/spin-out), rename, reset and
// delete, plus per-branch and repository-default configuration.
pub static BRANCH: TransientDef = TransientDef {
    title: "Branch",
    defaults: &[],
    incompatible: &[],
    groups: &[
        BRANCH_VARIABLES,
        GroupDef {
            title: "Configure repository defaults",
            items: &[
                Item::Variable {
                    key: "R",
                    var: "pull.rebase",
                },
                Item::Variable {
                    key: "P",
                    var: "remote.pushDefault",
                },
            ],
        },
        GroupDef {
            title: "Checkout",
            items: &[
                Item::Action {
                    key: "b",
                    desc: "branch/revision",
                    action: TransientAction::Checkout,
                },
                Item::Action {
                    key: "l",
                    desc: "local branch",
                    action: TransientAction::CheckoutLocal,
                },
                Item::Action {
                    key: "c",
                    desc: "new branch",
                    action: TransientAction::CreateCheckoutBranch,
                },
                Item::Action {
                    key: "s",
                    desc: "new spin-off",
                    action: TransientAction::BranchSpinoff,
                },
            ],
        },
        GroupDef {
            title: "Create",
            items: &[
                Item::Action {
                    key: "n",
                    desc: "new branch",
                    action: TransientAction::CreateBranch,
                },
                Item::Action {
                    key: "S",
                    desc: "new spin-out",
                    action: TransientAction::BranchSpinout,
                },
            ],
        },
        GroupDef {
            title: "Do",
            items: &[
                Item::Action {
                    key: "C",
                    desc: "configure...",
                    action: TransientAction::BranchConfigure,
                },
                Item::Action {
                    key: "m",
                    desc: "rename",
                    action: TransientAction::BranchRename,
                },
                Item::Action {
                    key: "x",
                    desc: "reset",
                    action: TransientAction::BranchReset,
                },
                Item::Action {
                    key: "k",
                    desc: "delete",
                    action: TransientAction::BranchDelete,
                },
            ],
        },
    ],
};

/// Variables menu for an explicitly picked branch.
pub static BRANCH_CONFIGURE: TransientDef = TransientDef {
    title: "Configure branch",
    defaults: &[],
    incompatible: &[],
    groups: &[BRANCH_VARIABLES],
};

// Arguments and actions mirror `magit-merge` (default levels), including
// the `--ff-only`/`--no-ff` incompatible pair.
pub static MERGE: TransientDef = TransientDef {
    title: "Merge",
    defaults: &[],
    incompatible: &[&["--ff-only", "--no-ff"]],
    groups: &[
        GroupDef {
            title: "Arguments",
            items: &[
                Item::Switch {
                    key: "-f",
                    flag: "--ff-only",
                    desc: "Fast-forward only",
                },
                Item::Switch {
                    key: "-n",
                    flag: "--no-ff",
                    desc: "No fast-forward",
                },
                Item::Arg {
                    key: "-A",
                    flag: "-Xdiff-algorithm=",
                    desc: "Diff algorithm",
                },
                Item::Arg {
                    key: "-S",
                    flag: "--gpg-sign=",
                    desc: "Sign using gpg",
                },
            ],
        },
        GroupDef {
            title: "Actions",
            items: &[
                Item::Action {
                    key: "m",
                    desc: "Merge",
                    action: TransientAction::Merge,
                },
                Item::Action {
                    key: "e",
                    desc: "Merge and edit message",
                    action: TransientAction::MergeEdit,
                },
                Item::Action {
                    key: "n",
                    desc: "Merge but don't commit",
                    action: TransientAction::MergeNoCommit,
                },
                Item::Action {
                    key: "a",
                    desc: "Absorb",
                    action: TransientAction::MergeAbsorb,
                },
                Item::Action {
                    key: "p",
                    desc: "Preview merge",
                    action: TransientAction::MergePreview,
                },
                Item::Action {
                    key: "s",
                    desc: "Squash merge",
                    action: TransientAction::MergeSquash,
                },
                Item::Action {
                    key: "i",
                    desc: "Merge into",
                    action: TransientAction::MergeInto,
                },
            ],
        },
    ],
};

/// Shown instead of `MERGE` while a merge is in progress (MERGE_HEAD
/// exists), mirroring Magit: starting another merge is impossible, so the
/// only sensible actions are finishing or aborting the current one.
pub static MERGE_IN_PROGRESS: TransientDef = TransientDef {
    title: "Merge (in progress)",
    defaults: &[],
    incompatible: &[],
    groups: &[GroupDef {
        title: "Actions",
        items: &[
            Item::Action {
                key: "m",
                desc: "Commit merge",
                action: TransientAction::Commit,
            },
            Item::Action {
                key: "a",
                desc: "Abort merge",
                action: TransientAction::MergeAbort,
            },
        ],
    }],
};

// Arguments mirror `magit-rebase` (default levels), including `--autostash`
// being on by default. `--interactive` is a real switch: enabling it makes
// the onto-upstream/elsewhere actions open the todo editor.
pub static REBASE: TransientDef = TransientDef {
    title: "Rebase",
    defaults: &["--autostash"],
    incompatible: &[],
    groups: &[
        GroupDef {
            title: "Arguments",
            items: &[
                Item::Switch {
                    key: "-k",
                    flag: "--keep-empty",
                    desc: "Keep empty commits",
                },
                Item::Switch {
                    key: "-d",
                    flag: "--committer-date-is-author-date",
                    desc: "Use author date as committer date",
                },
                Item::Switch {
                    key: "-t",
                    flag: "--ignore-date",
                    desc: "Use current time as author date",
                },
                Item::Switch {
                    key: "-a",
                    flag: "--autosquash",
                    desc: "Autosquash fixup and squash commits",
                },
                Item::Switch {
                    key: "-A",
                    flag: "--autostash",
                    desc: "Autostash",
                },
                Item::Switch {
                    key: "-i",
                    flag: "--interactive",
                    desc: "Interactive",
                },
                Item::Switch {
                    key: "-h",
                    flag: "--no-verify",
                    desc: "Disable hooks",
                },
            ],
        },
        GroupDef {
            title: "Actions",
            items: &[
                Item::Action {
                    key: "u",
                    desc: "Rebase onto upstream",
                    action: TransientAction::RebaseUpstream,
                },
                Item::Action {
                    key: "e",
                    desc: "Rebase onto elsewhere",
                    action: TransientAction::RebaseElsewhere,
                },
                Item::Action {
                    key: "i",
                    desc: "Rebase interactively",
                    action: TransientAction::RebaseInteractive,
                },
            ],
        },
    ],
};

/// Shown instead of `REBASE` while a rebase is in progress (rebase-merge or
/// rebase-apply exists), mirroring Magit: starting another rebase is
/// impossible, so the only sensible actions manage the current one.
pub static REBASE_IN_PROGRESS: TransientDef = TransientDef {
    title: "Rebase (in progress)",
    defaults: &[],
    incompatible: &[],
    groups: &[GroupDef {
        title: "Actions",
        items: &[
            Item::Action {
                key: "r",
                desc: "Continue",
                action: TransientAction::RebaseContinue,
            },
            Item::Action {
                key: "s",
                desc: "Skip this commit",
                action: TransientAction::RebaseSkip,
            },
            Item::Action {
                key: "e",
                desc: "Edit the todo list",
                action: TransientAction::RebaseEditTodo,
            },
            Item::Action {
                key: "a",
                desc: "Abort rebase",
                action: TransientAction::RebaseAbort,
            },
        ],
    }],
};

// Pick or apply commits onto HEAD. `--ff` starts enabled and is
// incompatible with `-x`: a fast-forward reuses the commit unchanged, so
// there is no new message to reference the cherry in.
pub static CHERRY_PICK: TransientDef = TransientDef {
    title: "Cherry-pick",
    defaults: &["--ff"],
    incompatible: &[&["--ff", "-x"]],
    groups: &[
        GroupDef {
            title: "Arguments",
            items: &[
                Item::Arg {
                    key: "-m",
                    flag: "--mainline=",
                    desc: "Replay merge relative to parent",
                },
                Item::Arg {
                    key: "=s",
                    flag: "--strategy=",
                    desc: "Strategy",
                },
                Item::Switch {
                    key: "-F",
                    flag: "--ff",
                    desc: "Attempt fast-forward",
                },
                Item::Switch {
                    key: "-x",
                    flag: "-x",
                    desc: "Reference cherry in commit message",
                },
                Item::Switch {
                    key: "-e",
                    flag: "--edit",
                    desc: "Edit commit messages",
                },
            ],
        },
        GroupDef {
            title: "Actions",
            items: &[
                Item::Action {
                    key: "A",
                    desc: "Pick",
                    action: TransientAction::CherryPick,
                },
                Item::Action {
                    key: "a",
                    desc: "Apply",
                    action: TransientAction::CherryPickApply,
                },
            ],
        },
    ],
};

/// Shown instead of `CHERRY_PICK` while a cherry-pick is stopped
/// (CHERRY_PICK_HEAD exists): the sequencer can only continue, skip or
/// abort.
pub static CHERRY_PICK_IN_PROGRESS: TransientDef = TransientDef {
    title: "Cherry-pick (in progress)",
    defaults: &[],
    incompatible: &[],
    groups: &[GroupDef {
        title: "Actions",
        items: &[
            Item::Action {
                key: "A",
                desc: "Continue",
                action: TransientAction::CherryPickContinue,
            },
            Item::Action {
                key: "s",
                desc: "Skip this commit",
                action: TransientAction::CherryPickSkip,
            },
            Item::Action {
                key: "a",
                desc: "Abort cherry-pick",
                action: TransientAction::CherryPickAbort,
            },
        ],
    }],
};

// Revert commits with a new commit or onto the worktree/index only;
// `--edit` starts enabled.
pub static REVERT: TransientDef = TransientDef {
    title: "Revert",
    defaults: &["--edit"],
    incompatible: &[],
    groups: &[
        GroupDef {
            title: "Arguments",
            items: &[
                Item::Arg {
                    key: "-m",
                    flag: "--mainline=",
                    desc: "Replay merge relative to parent",
                },
                Item::Switch {
                    key: "-e",
                    flag: "--edit",
                    desc: "Edit commit message",
                },
                Item::Switch {
                    key: "-E",
                    flag: "--no-edit",
                    desc: "Don't edit commit message",
                },
                Item::Arg {
                    key: "=s",
                    flag: "--strategy=",
                    desc: "Strategy",
                },
            ],
        },
        GroupDef {
            title: "Actions",
            items: &[
                Item::Action {
                    key: "V",
                    desc: "Revert commit",
                    action: TransientAction::Revert,
                },
                Item::Action {
                    key: "v",
                    desc: "Revert changes",
                    action: TransientAction::RevertNoCommit,
                },
            ],
        },
    ],
};

/// Shown instead of `REVERT` while a revert is stopped (REVERT_HEAD
/// exists): the sequencer can only continue, skip or abort.
pub static REVERT_IN_PROGRESS: TransientDef = TransientDef {
    title: "Revert (in progress)",
    defaults: &[],
    incompatible: &[],
    groups: &[GroupDef {
        title: "Actions",
        items: &[
            Item::Action {
                key: "V",
                desc: "Continue",
                action: TransientAction::RevertContinue,
            },
            Item::Action {
                key: "s",
                desc: "Skip this commit",
                action: TransientAction::RevertSkip,
            },
            Item::Action {
                key: "a",
                desc: "Abort revert",
                action: TransientAction::RevertAbort,
            },
        ],
    }],
};

// Move HEAD, the index and/or the worktree to a picked revision.
pub static RESET: TransientDef = TransientDef {
    title: "Reset",
    defaults: &[],
    incompatible: &[],
    groups: &[GroupDef {
        title: "Reset",
        items: &[
            Item::Action {
                key: "m",
                desc: "mixed    (HEAD and index)",
                action: TransientAction::ResetMixed,
            },
            Item::Action {
                key: "s",
                desc: "soft     (HEAD only)",
                action: TransientAction::ResetSoft,
            },
            Item::Action {
                key: "h",
                desc: "hard     (HEAD, index and worktree)",
                action: TransientAction::ResetHard,
            },
            Item::Action {
                key: "k",
                desc: "keep     (HEAD and index, keeping uncommitted)",
                action: TransientAction::ResetKeep,
            },
            Item::Action {
                key: "i",
                desc: "index    (only)",
                action: TransientAction::ResetIndex,
            },
            Item::Action {
                key: "w",
                desc: "worktree (only)",
                action: TransientAction::ResetWorktree,
            },
        ],
    }],
};

// Stash away index/worktree changes and apply/pop/drop existing stashes.
pub static STASH: TransientDef = TransientDef {
    title: "Stash",
    defaults: &[],
    incompatible: &[],
    groups: &[
        GroupDef {
            title: "Arguments",
            items: &[
                Item::Switch {
                    key: "-u",
                    flag: "--include-untracked",
                    desc: "Also save untracked files",
                },
                Item::Switch {
                    key: "-a",
                    flag: "--all",
                    desc: "Also save untracked and ignored files",
                },
            ],
        },
        GroupDef {
            title: "Stash",
            items: &[
                Item::Action {
                    key: "z",
                    desc: "both (index and worktree)",
                    action: TransientAction::StashBoth,
                },
                Item::Action {
                    key: "i",
                    desc: "index (staged changes only)",
                    action: TransientAction::StashIndex,
                },
                Item::Action {
                    key: "x",
                    desc: "keeping index",
                    action: TransientAction::StashKeepIndex,
                },
            ],
        },
        GroupDef {
            title: "Use",
            items: &[
                Item::Action {
                    key: "a",
                    desc: "Apply",
                    action: TransientAction::StashApply,
                },
                Item::Action {
                    key: "p",
                    desc: "Pop",
                    action: TransientAction::StashPop,
                },
                Item::Action {
                    key: "k",
                    desc: "Drop",
                    action: TransientAction::StashDrop,
                },
            ],
        },
    ],
};

// Create (optionally annotated/signed) and delete tags.
pub static TAG: TransientDef = TransientDef {
    title: "Tag",
    defaults: &[],
    incompatible: &[],
    groups: &[
        GroupDef {
            title: "Arguments",
            items: &[
                Item::Switch {
                    key: "-f",
                    flag: "--force",
                    desc: "Force",
                },
                Item::Switch {
                    key: "-e",
                    flag: "--edit",
                    desc: "Edit message",
                },
                Item::Switch {
                    key: "-a",
                    flag: "--annotate",
                    desc: "Annotate",
                },
                Item::Switch {
                    key: "-s",
                    flag: "--sign",
                    desc: "Sign",
                },
                Item::Arg {
                    key: "-u",
                    flag: "--local-user=",
                    desc: "Sign as",
                },
            ],
        },
        GroupDef {
            title: "Actions",
            items: &[
                Item::Action {
                    key: "t",
                    desc: "Create tag",
                    action: TransientAction::TagCreate,
                },
                Item::Action {
                    key: "k",
                    desc: "Delete tag",
                    action: TransientAction::TagDelete,
                },
            ],
        },
    ],
};

/// The per-remote variables, scoped to
/// the current remote in the remote menu and to a picked one in the
/// configure menu.
const REMOTE_VARIABLES: GroupDef = GroupDef {
    title: "Variables",
    items: &[
        Item::Variable {
            key: "u",
            var: "remote.{}.url",
        },
        Item::Variable {
            key: "U",
            var: "remote.{}.fetch",
        },
        Item::Variable {
            key: "s",
            var: "remote.{}.pushurl",
        },
        Item::Variable {
            key: "S",
            var: "remote.{}.push",
        },
        Item::Variable {
            key: "O",
            var: "remote.{}.tagOpt",
        },
        Item::Variable {
            key: "h",
            var: "remote.{}.followRemoteHEAD",
        },
    ],
};

// Add, configure or remove remotes; `-f` (fetch after add) starts
// enabled.
pub static REMOTE: TransientDef = TransientDef {
    title: "Remote",
    defaults: &["-f"],
    incompatible: &[],
    groups: &[
        REMOTE_VARIABLES,
        GroupDef {
            title: "Arguments for add",
            items: &[Item::Switch {
                key: "-f",
                flag: "-f",
                desc: "Fetch after add",
            }],
        },
        GroupDef {
            title: "Actions",
            items: &[
                Item::Action {
                    key: "a",
                    desc: "Add",
                    action: TransientAction::RemoteAdd,
                },
                Item::Action {
                    key: "r",
                    desc: "Rename",
                    action: TransientAction::RemoteRename,
                },
                Item::Action {
                    key: "k",
                    desc: "Remove",
                    action: TransientAction::RemoteRemove,
                },
                Item::Action {
                    key: "C",
                    desc: "Configure...",
                    action: TransientAction::RemoteConfigure,
                },
                Item::Action {
                    key: "p",
                    desc: "Prune stale branches",
                    action: TransientAction::RemotePrune,
                },
            ],
        },
    ],
};

/// Variables menu for an explicitly picked remote.
pub static REMOTE_CONFIGURE: TransientDef = TransientDef {
    title: "Configure remote",
    defaults: &[],
    incompatible: &[],
    groups: &[REMOTE_VARIABLES],
};

// Pull into the current branch; `--ff-only` and `--rebase` are mutually
// exclusive.
pub static PULL: TransientDef = TransientDef {
    title: "Pull",
    defaults: &[],
    incompatible: &[&["--ff-only", "--rebase"]],
    groups: &[
        GroupDef {
            title: "Arguments",
            items: &[
                Item::Switch {
                    key: "-f",
                    flag: "--ff-only",
                    desc: "Fast-forward only",
                },
                Item::Switch {
                    key: "-r",
                    flag: "--rebase",
                    desc: "Rebase local commits",
                },
                Item::Switch {
                    key: "-F",
                    flag: "--force",
                    desc: "Force",
                },
            ],
        },
        GroupDef {
            title: "Pull into current branch from",
            items: &[
                Item::Action {
                    key: "p",
                    desc: "push-remote",
                    action: TransientAction::PullFromPushRemote,
                },
                Item::Action {
                    key: "u",
                    desc: "upstream",
                    action: TransientAction::PullFromUpstream,
                },
                Item::Action {
                    key: "e",
                    desc: "elsewhere",
                    action: TransientAction::PullElsewhere,
                },
            ],
        },
        GroupDef {
            title: "Configure",
            items: &[
                Item::Variable {
                    key: "r",
                    var: "branch.{}.rebase",
                },
                Item::Action {
                    key: "C",
                    desc: "variables...",
                    action: TransientAction::BranchConfigure,
                },
            ],
        },
    ],
};

// Fetch from the push-remote/upstream, a picked remote or all remotes,
// down to a single branch or an explicit refspec.
pub static FETCH: TransientDef = TransientDef {
    title: "Fetch",
    defaults: &[],
    incompatible: &[],
    groups: &[
        GroupDef {
            title: "Arguments",
            items: &[
                Item::Switch {
                    key: "-p",
                    flag: "--prune",
                    desc: "Prune deleted branches",
                },
                Item::Switch {
                    key: "-t",
                    flag: "--tags",
                    desc: "Fetch all tags",
                },
                Item::Switch {
                    key: "-F",
                    flag: "--force",
                    desc: "Force",
                },
            ],
        },
        GroupDef {
            title: "Fetch from",
            items: &[
                Item::Action {
                    key: "p",
                    desc: "push-remote",
                    action: TransientAction::FetchFromPushRemote,
                },
                Item::Action {
                    key: "u",
                    desc: "upstream",
                    action: TransientAction::FetchFromUpstream,
                },
                Item::Action {
                    key: "e",
                    desc: "elsewhere",
                    action: TransientAction::FetchElsewhere,
                },
                Item::Action {
                    key: "a",
                    desc: "all remotes",
                    action: TransientAction::FetchAll,
                },
            ],
        },
        GroupDef {
            title: "Fetch",
            items: &[
                Item::Action {
                    key: "o",
                    desc: "another branch",
                    action: TransientAction::FetchBranch,
                },
                Item::Action {
                    key: "r",
                    desc: "explicit refspec",
                    action: TransientAction::FetchRefspec,
                },
            ],
        },
        GroupDef {
            title: "Configure",
            items: &[Item::Action {
                key: "C",
                desc: "variables...",
                action: TransientAction::BranchConfigure,
            }],
        },
    ],
};

pub static LOG: TransientDef = TransientDef {
    title: "Log",
    defaults: &[],
    incompatible: &[],
    groups: &[
        GroupDef {
            title: "Arguments",
            items: &[
                // --graph is intentionally omitted: it prefixes graph art to
                // each line, which our `--format` field parser can't read.
                Item::Arg {
                    key: "-n",
                    flag: "--max-count=",
                    desc: "Limit number of commits",
                },
                Item::Arg {
                    key: "-A",
                    flag: "--author=",
                    desc: "Limit to author",
                },
                Item::Arg {
                    key: "-F",
                    flag: "--grep=",
                    desc: "Search commit messages",
                },
                Item::Switch {
                    key: "-m",
                    flag: "--no-merges",
                    desc: "Omit merge commits",
                },
                Item::Switch {
                    key: "-r",
                    flag: "--reverse",
                    desc: "Show oldest first",
                },
                Item::Switch {
                    key: "-f",
                    flag: "--first-parent",
                    desc: "Follow first parent only",
                },
            ],
        },
        GroupDef {
            title: "Actions",
            items: &[
                Item::Action {
                    key: "l",
                    desc: "Log current branch",
                    action: TransientAction::LogCurrent,
                },
                Item::Action {
                    key: "o",
                    desc: "Log other branch/revision",
                    action: TransientAction::LogOther,
                },
                Item::Action {
                    key: "a",
                    desc: "Log all references",
                    action: TransientAction::LogAll,
                },
            ],
        },
    ],
};

/// Resolve a `Command::Transient(menu)` to its definition. A new menu adds
/// one arm here and nothing in `App::dispatch`. Menus whose contents depend
/// on repo state are swapped in `App::open_transient` (merge or rebase in
/// progress).
pub fn menu_def(menu: Menu) -> &'static TransientDef {
    match menu {
        Menu::Commit => &COMMIT,
        Menu::Branch => &BRANCH,
        Menu::Merge => &MERGE,
        Menu::Rebase => &REBASE,
        Menu::CherryPick => &CHERRY_PICK,
        Menu::Revert => &REVERT,
        Menu::Reset => &RESET,
        Menu::Stash => &STASH,
        Menu::Tag => &TAG,
        Menu::Remote => &REMOTE,
        Menu::Push => &PUSH,
        Menu::Pull => &PULL,
        Menu::Fetch => &FETCH,
        Menu::Log => &LOG,
    }
}

/// A currently-open transient: the definition plus toggled switches and the
/// multi-char key input buffer (switch keys like "-a" are two keystrokes).
#[derive(Debug, Clone)]
pub struct TransientState {
    pub def: &'static TransientDef,
    pub enabled: BTreeSet<&'static str>,
    /// Value arguments (`--author=` → "ada"), set via a value prompt.
    pub values: BTreeMap<&'static str, String>,
    /// What `{}` in variable items resolves to (a branch or remote name).
    pub scope: Option<String>,
    /// Current values of the definition's variable items, keyed by the
    /// unresolved template; absent means unset. The app loads these when
    /// the menu opens and after each edit.
    pub variables: BTreeMap<&'static str, String>,
    /// Variables with a fixed set of values: all choices render inline
    /// and the key cycles through them instead of prompting. Loaded by
    /// the app.
    pub var_choices: BTreeMap<&'static str, Vec<String>>,
    /// The trailing `[...]` segment for a choice variable — the fallback
    /// variable's current value ("pull.rebase:true") or the built-in
    /// default ("default:false") — highlighted while the variable is unset.
    pub var_fallbacks: BTreeMap<&'static str, String>,
    pub pending: String,
}

/// Outcome of feeding one key to an open transient.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransientResult {
    /// Key consumed (switch toggled or prefix pending); keep the menu open.
    Consumed,
    /// Prompt for this value argument's value; the menu stays open.
    Prompt {
        flag: &'static str,
        desc: &'static str,
    },
    /// Prompt for a new value of this config variable; the menu stays open.
    EditVariable { var: &'static str },
    /// Run this action with these collected CLI flags; menu closes.
    Invoke(TransientAction, Vec<String>),
    /// Close without running anything.
    Cancel,
    /// Key didn't match any item.
    Unbound,
}

impl TransientState {
    pub fn new(def: &'static TransientDef) -> Self {
        Self {
            def,
            enabled: def.defaults.iter().copied().collect(),
            values: BTreeMap::new(),
            scope: None,
            variables: BTreeMap::new(),
            var_choices: BTreeMap::new(),
            var_fallbacks: BTreeMap::new(),
            pending: String::new(),
        }
    }

    /// Set (or clear, when empty) a value argument's value.
    pub fn set_value(&mut self, flag: &'static str, value: String) {
        if value.is_empty() {
            self.values.remove(flag);
        } else {
            self.values.insert(flag, value);
        }
    }

    /// Record a variable's (re-read) value; `None` means unset.
    pub fn set_variable(&mut self, var: &'static str, value: Option<String>) {
        match value {
            Some(v) => {
                self.variables.insert(var, v);
            }
            None => {
                self.variables.remove(var);
            }
        }
    }

    /// Scoped variables only make sense once there is a scope.
    fn item_visible(&self, item: &Item) -> bool {
        !matches!(item, Item::Variable { var, .. }
            if var.contains("{}") && self.scope.is_none())
    }

    pub fn args(&self) -> Vec<String> {
        let mut out: Vec<String> = self.enabled.iter().map(|f| f.to_string()).collect();
        out.extend(self.values.iter().map(|(flag, val)| format!("{flag}{val}")));
        out
    }

    pub fn on_key(&mut self, kp: &KeyPress) -> TransientResult {
        if kp.is_esc()
            || (kp.code == KeyCode::Char('g')
                && kp
                    .mods
                    .contains(ratatui::crossterm::event::KeyModifiers::CONTROL))
        {
            if self.pending.is_empty() {
                return TransientResult::Cancel;
            }
            self.pending.clear();
            return TransientResult::Consumed;
        }
        let KeyCode::Char(c) = kp.code else {
            return TransientResult::Unbound;
        };
        self.pending.push(c);

        let items = || {
            self.def
                .groups
                .iter()
                .flat_map(|g| g.items.iter())
                .filter(|i| self.item_visible(i))
        };
        if let Some(item) = items().find(|i| i.key() == self.pending) {
            self.pending.clear();
            match *item {
                Item::Switch { flag, .. } => {
                    if !self.enabled.remove(flag) {
                        self.enabled.insert(flag);
                        // Enabling a switch clears the rest of its
                        // incompatible set.
                        for set in self.def.incompatible {
                            if set.contains(&flag) {
                                for other in set.iter().filter(|o| **o != flag) {
                                    self.enabled.remove(other);
                                }
                            }
                        }
                    }
                    TransientResult::Consumed
                }
                Item::Arg { flag, desc, .. } => TransientResult::Prompt { flag, desc },
                Item::Action { action, .. } => TransientResult::Invoke(action, self.args()),
                Item::Variable { var, .. } => TransientResult::EditVariable { var },
            }
        } else if items().any(|i| i.key().starts_with(&self.pending)) {
            TransientResult::Consumed
        } else {
            self.pending.clear();
            TransientResult::Unbound
        }
    }

    /// Lines for the bottom panel.
    pub fn render_lines(&self, t: &Theme) -> Vec<Line<'static>> {
        let mut out = Vec::new();
        for group in self.def.groups {
            // A group can lose all its items (scoped variables without a
            // scope); drop its title too.
            let items: Vec<&Item> = group
                .items
                .iter()
                .filter(|i| self.item_visible(i))
                .collect();
            if items.is_empty() {
                continue;
            }
            out.push(Line::from(Span::styled(
                group.title.to_string(),
                Style::new().fg(t.menu_title).add_modifier(Modifier::BOLD),
            )));
            for item in items {
                match *item {
                    Item::Switch { key, flag, desc } => {
                        let on = self.enabled.contains(flag);
                        let flag_style = if on {
                            Style::new().fg(t.command).bold()
                        } else {
                            Style::new().dim()
                        };
                        out.push(Line::from(vec![
                            Span::raw(" "),
                            Span::styled(format!("{key:<4}"), Style::new().fg(t.key)),
                            Span::styled(format!("{flag:<22}"), flag_style),
                            Span::raw(desc.to_string()),
                        ]));
                    }
                    Item::Arg { key, flag, desc } => {
                        let (shown, style) = match self.values.get(flag) {
                            Some(v) => (format!("{flag}{v}"), Style::new().fg(t.command).bold()),
                            None => (flag.to_string(), Style::new().dim()),
                        };
                        out.push(Line::from(vec![
                            Span::raw(" "),
                            Span::styled(format!("{key:<4}"), Style::new().fg(t.key)),
                            Span::styled(format!("{shown:<22}"), style),
                            Span::raw(desc.to_string()),
                        ]));
                    }
                    Item::Action { key, desc, .. } => {
                        out.push(Line::from(vec![
                            Span::raw(" "),
                            Span::styled(format!("{key:<4}"), Style::new().fg(t.key)),
                            Span::raw(desc.to_string()),
                        ]));
                    }
                    Item::Variable { key, var } => {
                        let name = resolve_var(var, self.scope.as_deref().unwrap_or(""));
                        let mut spans = vec![
                            Span::raw(" "),
                            Span::styled(format!("{key:<4}"), Style::new().fg(t.key)),
                            Span::raw(format!("{name:<32}")),
                        ];
                        let current = self.variables.get(var);
                        let on = Style::new().fg(t.command).bold();
                        let off = Style::new().dim();
                        if let Some(choices) = self.var_choices.get(var) {
                            // All choices inline, the active one
                            // highlighted ([true|false|default:false]).
                            spans.push(Span::styled("[".to_string(), off));
                            for (i, choice) in choices.iter().enumerate() {
                                if i > 0 {
                                    spans.push(Span::styled("|".to_string(), off));
                                }
                                let style = if current == Some(choice) { on } else { off };
                                spans.push(Span::styled(choice.clone(), style));
                            }
                            if let Some(fallback) = self.var_fallbacks.get(var) {
                                spans.push(Span::styled("|".to_string(), off));
                                let style = if current.is_none() { on } else { off };
                                spans.push(Span::styled(fallback.clone(), style));
                            }
                            spans.push(Span::styled("]".to_string(), off));
                        } else {
                            let (shown, style) = match current {
                                Some(v) => (v.clone(), on),
                                None => ("unset".to_string(), off),
                            };
                            spans.push(Span::styled(shown, style));
                        }
                        out.push(Line::from(spans));
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyModifiers;

    fn key(c: char) -> KeyPress {
        KeyPress::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn switch_key_is_a_two_key_sequence() {
        let mut st = TransientState::new(&COMMIT);
        assert_eq!(st.on_key(&key('-')), TransientResult::Consumed);
        assert_eq!(st.on_key(&key('a')), TransientResult::Consumed);
        assert!(st.enabled.contains("--all"));
        // Toggling off again.
        st.on_key(&key('-'));
        st.on_key(&key('a'));
        assert!(st.enabled.is_empty());
    }

    #[test]
    fn action_collects_enabled_flags() {
        let mut st = TransientState::new(&COMMIT);
        st.on_key(&key('-'));
        st.on_key(&key('s'));
        match st.on_key(&key('c')) {
            TransientResult::Invoke(TransientAction::Commit, args) => {
                assert_eq!(args, vec!["--signoff"]);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn value_arg_prompts_then_collects_flag_with_value() {
        let mut st = TransientState::new(&LOG);
        // "-n" is a two-key sequence that requests a value prompt.
        assert_eq!(st.on_key(&key('-')), TransientResult::Consumed);
        assert_eq!(
            st.on_key(&key('n')),
            TransientResult::Prompt {
                flag: "--max-count=",
                desc: "Limit number of commits",
            }
        );
        st.set_value("--max-count=", "50".into());
        // A boolean switch alongside it.
        st.on_key(&key('-'));
        st.on_key(&key('m'));
        match st.on_key(&key('l')) {
            TransientResult::Invoke(TransientAction::LogCurrent, args) => {
                assert_eq!(args, vec!["--no-merges", "--max-count=50"]);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn empty_value_clears_the_arg() {
        let mut st = TransientState::new(&LOG);
        st.set_value("--author=", "ada".into());
        assert_eq!(st.args(), vec!["--author=ada"]);
        st.set_value("--author=", String::new());
        assert!(st.args().is_empty());
    }

    #[test]
    fn switch_and_action_sharing_a_letter_stay_distinct() {
        // MERGE has both the "-n" switch and the "n" action; the pending
        // prefix must keep the two-key sequence separate from the action.
        let mut st = TransientState::new(&MERGE);
        st.on_key(&key('-'));
        st.on_key(&key('n'));
        assert!(st.enabled.contains("--no-ff"));
        match st.on_key(&key('n')) {
            TransientResult::Invoke(TransientAction::MergeNoCommit, args) => {
                assert_eq!(args, vec!["--no-ff"]);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn in_progress_merge_menu_only_finishes_or_aborts() {
        let mut st = TransientState::new(&MERGE_IN_PROGRESS);
        assert_eq!(
            st.on_key(&key('m')),
            TransientResult::Invoke(TransientAction::Commit, vec![])
        );
        assert_eq!(
            st.on_key(&key('a')),
            TransientResult::Invoke(TransientAction::MergeAbort, vec![])
        );
        // Actions from the normal merge menu are gone.
        assert_eq!(st.on_key(&key('s')), TransientResult::Unbound);
    }

    #[test]
    fn ff_only_and_no_ff_are_mutually_exclusive_like_magit() {
        let mut st = TransientState::new(&MERGE);
        st.on_key(&key('-'));
        st.on_key(&key('f'));
        assert!(st.enabled.contains("--ff-only"));
        // Enabling the other side of the pair clears the first.
        st.on_key(&key('-'));
        st.on_key(&key('n'));
        assert!(st.enabled.contains("--no-ff"));
        assert!(!st.enabled.contains("--ff-only"));
    }

    #[test]
    fn rebase_menu_defaults_to_autostash_like_magit() {
        let mut st = TransientState::new(&REBASE);
        match st.on_key(&key('u')) {
            TransientResult::Invoke(TransientAction::RebaseUpstream, args) => {
                assert_eq!(args, vec!["--autostash"]);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn rebase_action_collects_enabled_flags() {
        let mut st = TransientState::new(&REBASE);
        // A default switch toggles off like any other; others toggle on.
        st.on_key(&key('-'));
        st.on_key(&key('A'));
        st.on_key(&key('-'));
        st.on_key(&key('i'));
        match st.on_key(&key('u')) {
            TransientResult::Invoke(TransientAction::RebaseUpstream, args) => {
                assert_eq!(args, vec!["--interactive"]);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn in_progress_rebase_menu_only_manages_the_current_rebase() {
        let mut st = TransientState::new(&REBASE_IN_PROGRESS);
        assert_eq!(
            st.on_key(&key('r')),
            TransientResult::Invoke(TransientAction::RebaseContinue, vec![])
        );
        assert_eq!(
            st.on_key(&key('a')),
            TransientResult::Invoke(TransientAction::RebaseAbort, vec![])
        );
        // Starting a new rebase is not offered while one is in progress.
        assert_eq!(st.on_key(&key('u')), TransientResult::Unbound);
    }

    #[test]
    fn cherry_pick_menu_defaults_to_ff() {
        let mut st = TransientState::new(&CHERRY_PICK);
        match st.on_key(&key('A')) {
            TransientResult::Invoke(TransientAction::CherryPick, args) => {
                assert_eq!(args, vec!["--ff"]);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn cherry_pick_ff_and_record_origin_are_mutually_exclusive() {
        let mut st = TransientState::new(&CHERRY_PICK);
        // Enabling -x clears the default --ff via the incompatible pair.
        st.on_key(&key('-'));
        st.on_key(&key('x'));
        match st.on_key(&key('A')) {
            TransientResult::Invoke(TransientAction::CherryPick, args) => {
                assert_eq!(args, vec!["-x"]);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn in_progress_cherry_pick_menu_only_manages_the_sequence() {
        let mut st = TransientState::new(&CHERRY_PICK_IN_PROGRESS);
        assert_eq!(
            st.on_key(&key('A')),
            TransientResult::Invoke(TransientAction::CherryPickContinue, vec![])
        );
        assert_eq!(
            st.on_key(&key('a')),
            TransientResult::Invoke(TransientAction::CherryPickAbort, vec![])
        );
        // Starting a new cherry-pick is not offered while one is stopped.
        assert_eq!(st.on_key(&key('e')), TransientResult::Unbound);
    }

    #[test]
    fn revert_menu_defaults_to_edit() {
        let mut st = TransientState::new(&REVERT);
        match st.on_key(&key('V')) {
            TransientResult::Invoke(TransientAction::Revert, args) => {
                assert_eq!(args, vec!["--edit"]);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn in_progress_revert_menu_only_manages_the_sequence() {
        let mut st = TransientState::new(&REVERT_IN_PROGRESS);
        assert_eq!(
            st.on_key(&key('V')),
            TransientResult::Invoke(TransientAction::RevertContinue, vec![])
        );
        assert_eq!(
            st.on_key(&key('a')),
            TransientResult::Invoke(TransientAction::RevertAbort, vec![])
        );
        // Starting a new revert is not offered while one is stopped.
        assert_eq!(st.on_key(&key('v')), TransientResult::Unbound);
    }

    #[test]
    fn variables_hide_without_scope_and_edit_with_one() {
        static DEF: TransientDef = TransientDef {
            title: "T",
            defaults: &[],
            incompatible: &[],
            groups: &[GroupDef {
                title: "Variables",
                items: &[Item::Variable {
                    key: "u",
                    var: "remote.{}.url",
                }],
            }],
        };
        let mut st = TransientState::new(&DEF);
        // Without a scope the variable is hidden and its key unbound.
        assert_eq!(st.on_key(&key('u')), TransientResult::Unbound);
        assert!(st.render_lines(&Theme::default()).is_empty());
        st.scope = Some("origin".into());
        assert_eq!(
            st.on_key(&key('u')),
            TransientResult::EditVariable {
                var: "remote.{}.url"
            }
        );
        assert!(!st.render_lines(&Theme::default()).is_empty());
    }

    #[test]
    fn choice_variable_renders_all_choices() {
        static DEF: TransientDef = TransientDef {
            title: "T",
            defaults: &[],
            incompatible: &[],
            groups: &[GroupDef {
                title: "Variables",
                items: &[Item::Variable {
                    key: "r",
                    var: "branch.{}.rebase",
                }],
            }],
        };
        let mut st = TransientState::new(&DEF);
        st.scope = Some("main".into());
        st.var_choices
            .insert("branch.{}.rebase", vec!["true".into(), "false".into()]);
        st.var_fallbacks
            .insert("branch.{}.rebase", "default:false".into());
        st.variables.insert("branch.{}.rebase", "true".into());
        let lines = st.render_lines(&Theme::default());
        let text: String = lines[1].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("[true|false|default:false]"), "got {text:?}");
    }

    #[test]
    fn resolve_var_substitutes_the_scope() {
        assert_eq!(resolve_var("remote.{}.url", "origin"), "remote.origin.url");
        assert_eq!(resolve_var("pull.rebase", "main"), "pull.rebase");
    }

    #[test]
    fn esc_cancels() {
        let mut st = TransientState::new(&PUSH);
        let esc = KeyPress::new(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(st.on_key(&esc), TransientResult::Cancel);
    }

    #[test]
    fn unknown_key_is_unbound() {
        let mut st = TransientState::new(&PUSH);
        assert_eq!(st.on_key(&key('z')), TransientResult::Unbound);
        assert!(st.pending.is_empty());
    }
}
