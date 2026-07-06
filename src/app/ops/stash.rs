//! The stash transient: push variants run immediately with the collected
//! flags; apply/pop/drop go through a picker over the stash list that
//! defaults to the stash at point. Drop confirms first, since the stash is
//! lost.

use crate::app::{svec, App, Confirm, PendingAction};
use crate::ui::section::SectionValue;
use crate::ui::transient::TransientAction;

impl App {
    pub(super) fn stash_action(&mut self, action: TransientAction, args: Vec<String>) {
        let prompt = match action {
            TransientAction::StashBoth => {
                let mut cli = svec(&["stash", "push"]);
                cli.extend(args);
                self.run_git_bg("stash".into(), cli, None);
                return;
            }
            TransientAction::StashIndex => {
                // Untracked files are never staged, so git rejects
                // --include-untracked/--all alongside --staged.
                let mut cli = svec(&["stash", "push", "--staged"]);
                cli.extend(
                    args.into_iter()
                        .filter(|f| f != "--include-untracked" && f != "--all"),
                );
                self.run_git_bg("stash index".into(), cli, None);
                return;
            }
            TransientAction::StashKeepIndex => {
                let mut cli = svec(&["stash", "push", "--keep-index"]);
                cli.extend(args);
                self.run_git_bg("stash keeping index".into(), cli, None);
                return;
            }
            TransientAction::StashApply => "Apply stash",
            TransientAction::StashPop => "Pop stash",
            TransientAction::StashDrop => "Drop stash",
            _ => unreachable!("not a stash action"),
        };
        let stashes = self.list_stashes_at_point();
        self.open_strict_picker(prompt, stashes, "no stashes", move |app, value| {
            // Candidates read "stash@{N}: message"; peel the message back
            // off.
            let stash = value
                .split_once(':')
                .map(|(r, _)| r)
                .filter(|r| r.starts_with("stash@{"))
                .unwrap_or(&value)
                .to_string();
            match action {
                TransientAction::StashApply => {
                    app.run_git_bg(
                        format!("apply {stash}"),
                        svec(&["stash", "apply", &stash]),
                        None,
                    );
                }
                TransientAction::StashPop => {
                    app.run_git_bg(
                        format!("pop {stash}"),
                        svec(&["stash", "pop", &stash]),
                        None,
                    );
                }
                TransientAction::StashDrop => {
                    app.confirm = Some(Confirm {
                        prompt: format!("Drop {stash}?"),
                        action: PendingAction::Git {
                            desc: format!("drop {stash}"),
                            args: svec(&["stash", "drop", &stash]),
                            stdin: None,
                        },
                    });
                }
                _ => unreachable!("not a stash action"),
            }
        });
    }

    /// The stash list as picker candidates ("stash@{N}: message"), with the
    /// stash at point (if any) first.
    fn list_stashes_at_point(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .snapshot
            .as_ref()
            .map(|s| {
                s.stashes
                    .iter()
                    .map(|st| format!("stash@{{{}}}: {}", st.index, st.message))
                    .collect()
            })
            .unwrap_or_default();
        if let Some(SectionValue::Stash { index }) = self.panes.last().map(|p| p.value_at_cursor())
        {
            let prefix = format!("stash@{{{index}}}:");
            if let Some(i) = out.iter().position(|c| c.starts_with(&prefix)) {
                let at_point = out.remove(i);
                out.insert(0, at_point);
            }
        }
        out
    }
}
