//! The worktree transient: check out an existing branch or a freshly created
//! one in a new linked worktree, move a worktree, or delete one. Delete
//! confirms first since it removes a checked-out working tree. The `--force`
//! switch overrides git's safety checks on add/move/remove.

use crate::app::{svec, App, Confirm, PendingAction};
use crate::ui::transient::TransientAction;

impl App {
    pub(super) fn worktree_action(&mut self, action: TransientAction, args: Vec<String>) {
        let force = args.iter().any(|a| a == "--force");
        match action {
            TransientAction::WorktreeCheckout => {
                // Ask for the new worktree path, then the branch to check out.
                self.open_input("Create worktree at path", move |app, path| {
                    let branches = app.list_branches();
                    app.open_picker(
                        format!("Checkout branch in {path}"),
                        branches,
                        move |app, branch| {
                            let mut cli = svec(&["worktree", "add"]);
                            if force {
                                cli.push("--force".into());
                            }
                            cli.push(path.clone());
                            cli.push(branch.clone());
                            app.run_git_bg(format!("worktree {path} checkout {branch}"), cli, None);
                        },
                    );
                });
            }
            TransientAction::WorktreeBranch => {
                // New branch name → its start point → the worktree path.
                self.open_input("New branch name", move |app, name| {
                    let starts = app.list_revs_at_point();
                    app.open_picker(
                        format!("Create branch {name} starting at"),
                        starts,
                        move |app, start| {
                            app.open_input("Create worktree at path", move |app, path| {
                                let mut cli = svec(&["worktree", "add"]);
                                if force {
                                    cli.push("--force".into());
                                }
                                cli.push("-b".into());
                                cli.push(name.clone());
                                cli.push(path.clone());
                                cli.push(start.clone());
                                app.run_git_bg(
                                    format!("worktree {path} new branch {name}"),
                                    cli,
                                    None,
                                );
                            });
                        },
                    );
                });
            }
            TransientAction::WorktreeMove => {
                let worktrees = self.list_linked_worktrees();
                self.open_strict_picker(
                    "Move worktree",
                    worktrees,
                    "no linked worktrees",
                    move |app, path| {
                        app.open_input(format!("Move {path} to"), move |app, dest| {
                            let mut cli = svec(&["worktree", "move"]);
                            if force {
                                cli.push("--force".into());
                            }
                            cli.push(path.clone());
                            cli.push(dest.clone());
                            app.run_git_bg(format!("move worktree {path} to {dest}"), cli, None);
                        });
                    },
                );
            }
            TransientAction::WorktreeDelete => {
                let worktrees = self.list_linked_worktrees();
                self.open_strict_picker(
                    "Delete worktree",
                    worktrees,
                    "no linked worktrees",
                    move |app, path| {
                        let mut cli = svec(&["worktree", "remove"]);
                        if force {
                            cli.push("--force".into());
                        }
                        cli.push(path.clone());
                        app.confirm = Some(Confirm {
                            prompt: format!("Delete worktree {path}?"),
                            action: PendingAction::Git {
                                desc: format!("remove worktree {path}"),
                                args: cli,
                                stdin: None,
                            },
                        });
                    },
                );
            }
            _ => unreachable!("not a worktree action"),
        }
    }

    /// Linked worktree paths (the main worktree excluded), for the move/delete
    /// pickers. `git worktree list --porcelain` lists the main worktree first.
    pub(super) fn list_linked_worktrees(&self) -> Vec<String> {
        let mut paths: Vec<String> = self
            .git
            .run(&["worktree", "list", "--porcelain"])
            .map(|o| o.stdout)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.strip_prefix("worktree ").map(str::to_string))
            .collect();
        if !paths.is_empty() {
            paths.remove(0);
        }
        paths
    }
}
