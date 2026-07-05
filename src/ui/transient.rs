//! Data-driven transient menus (Magit's popup system). A transient is pure
//! data: groups of switches and actions. While one is open it captures all
//! keys; actions collect the enabled switches into CLI flags.

use std::collections::{BTreeMap, BTreeSet};

use ratatui::crossterm::event::KeyCode;
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};

use crate::keymap::KeyPress;
use crate::theme::Theme;

/// What an action ultimately runs. The app maps these to git invocations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransientAction {
    Commit,
    CommitAmend,
    CommitExtend,
    Push,
    PushSetUpstream,
    Pull,
    Fetch,
    FetchAll,
    /// Opens a picker over local and remote branches.
    Checkout,
    /// Opens a minibuffer for the new branch name, then checks it out.
    CreateCheckoutBranch,
    /// Opens a minibuffer for the new branch name (no checkout).
    CreateBranch,
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
}

impl Item {
    fn key(&self) -> &'static str {
        match self {
            Item::Switch { key, .. } | Item::Arg { key, .. } | Item::Action { key, .. } => key,
        }
    }
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
}

pub static COMMIT: TransientDef = TransientDef {
    title: "Commit",
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

pub static PUSH: TransientDef = TransientDef {
    title: "Push",
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
                    key: "-n",
                    flag: "--dry-run",
                    desc: "Dry run",
                },
            ],
        },
        GroupDef {
            title: "Actions",
            items: &[
                Item::Action {
                    key: "p",
                    desc: "Push to upstream",
                    action: TransientAction::Push,
                },
                Item::Action {
                    key: "u",
                    desc: "Push and set upstream (origin HEAD)",
                    action: TransientAction::PushSetUpstream,
                },
            ],
        },
    ],
};

pub static BRANCH: TransientDef = TransientDef {
    title: "Branch",
    groups: &[GroupDef {
        title: "Actions",
        items: &[
            Item::Action {
                key: "b",
                desc: "Checkout branch/revision",
                action: TransientAction::Checkout,
            },
            Item::Action {
                key: "c",
                desc: "Create new branch and checkout",
                action: TransientAction::CreateCheckoutBranch,
            },
            Item::Action {
                key: "n",
                desc: "Create new branch",
                action: TransientAction::CreateBranch,
            },
        ],
    }],
};

pub static PULL: TransientDef = TransientDef {
    title: "Pull",
    groups: &[
        GroupDef {
            title: "Arguments",
            items: &[
                Item::Switch {
                    key: "-r",
                    flag: "--rebase",
                    desc: "Rebase local commits",
                },
                Item::Switch {
                    key: "-a",
                    flag: "--autostash",
                    desc: "Autostash",
                },
            ],
        },
        GroupDef {
            title: "Actions",
            items: &[Item::Action {
                key: "u",
                desc: "Pull from upstream",
                action: TransientAction::Pull,
            }],
        },
    ],
};

pub static FETCH: TransientDef = TransientDef {
    title: "Fetch",
    groups: &[
        GroupDef {
            title: "Arguments",
            items: &[Item::Switch {
                key: "-p",
                flag: "--prune",
                desc: "Prune deleted branches",
            }],
        },
        GroupDef {
            title: "Actions",
            items: &[
                Item::Action {
                    key: "u",
                    desc: "Fetch from upstream",
                    action: TransientAction::Fetch,
                },
                Item::Action {
                    key: "a",
                    desc: "Fetch from all remotes",
                    action: TransientAction::FetchAll,
                },
            ],
        },
    ],
};

pub static LOG: TransientDef = TransientDef {
    title: "Log",
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

/// A currently-open transient: the definition plus toggled switches and the
/// multi-char key input buffer (switch keys like "-a" are two keystrokes).
#[derive(Debug, Clone)]
pub struct TransientState {
    pub def: &'static TransientDef,
    pub enabled: BTreeSet<&'static str>,
    /// Value arguments (`--author=` → "ada"), set via a value prompt.
    pub values: BTreeMap<&'static str, String>,
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
            enabled: BTreeSet::new(),
            values: BTreeMap::new(),
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

        let items = || self.def.groups.iter().flat_map(|g| g.items.iter());
        if let Some(item) = items().find(|i| i.key() == self.pending) {
            self.pending.clear();
            match *item {
                Item::Switch { flag, .. } => {
                    if !self.enabled.remove(flag) {
                        self.enabled.insert(flag);
                    }
                    TransientResult::Consumed
                }
                Item::Arg { flag, desc, .. } => TransientResult::Prompt { flag, desc },
                Item::Action { action, .. } => TransientResult::Invoke(action, self.args()),
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
            out.push(Line::from(Span::styled(
                group.title.to_string(),
                Style::new().fg(t.menu_title).add_modifier(Modifier::BOLD),
            )));
            for item in group.items {
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
