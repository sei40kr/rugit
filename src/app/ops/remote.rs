//! The push / pull / fetch transients (fire-and-forget mutations against
//! remotes, all running on worker threads) and the remote menu
//! (add/rename/remove/prune, chaining minibuffer inputs like magit-remote).

use crate::app::{svec, App};
use crate::ui::transient::TransientAction;

impl App {
    pub(super) fn remote_action(&mut self, action: TransientAction, mut args: Vec<String>) {
        match action {
            TransientAction::Push => {
                let mut a = svec(&["push"]);
                a.append(&mut args);
                self.run_git_bg("push".into(), a, None);
            }
            TransientAction::PushSetUpstream => {
                let mut a = svec(&["push", "-u", "origin", "HEAD"]);
                a.append(&mut args);
                self.run_git_bg("push (set upstream)".into(), a, None);
            }
            TransientAction::Pull => {
                let mut a = svec(&["pull"]);
                a.append(&mut args);
                self.run_git_bg("pull".into(), a, None);
            }
            TransientAction::Fetch => {
                let mut a = svec(&["fetch"]);
                a.append(&mut args);
                self.run_git_bg("fetch".into(), a, None);
            }
            TransientAction::FetchAll => {
                let mut a = svec(&["fetch", "--all"]);
                a.append(&mut args);
                self.run_git_bg("fetch --all".into(), a, None);
            }
            _ => unreachable!("not a remote action"),
        }
    }

    /// The current branch's remote, falling back to the sole remote;
    /// scopes the remote menu's variables.
    pub(super) fn current_remote(&self) -> Option<String> {
        self.snapshot
            .as_ref()
            .and_then(|s| s.branch.head.clone())
            .and_then(|b| {
                self.git
                    .run(&["config", "--get", &format!("branch.{b}.remote")])
                    .ok()
                    .filter(|o| o.status == 0)
                    .map(|o| o.stdout.trim().to_string())
                    .filter(|s| !s.is_empty())
            })
            .filter(|r| r != ".")
            .or_else(|| {
                let remotes = self.list_remotes();
                (remotes.len() == 1).then(|| remotes[0].clone())
            })
    }

    pub(super) fn remote_menu_action(&mut self, action: TransientAction, args: Vec<String>) {
        match action {
            TransientAction::RemoteConfigure => {
                self.pick_remote("Configure remote", |app, remote| {
                    app.open_configure_transient(&crate::ui::transient::REMOTE_CONFIGURE, remote);
                });
            }
            TransientAction::RemoteAdd => {
                // Ask for the name, then the URL; the -f flag rides along.
                self.open_input("Remote name", move |app, name| {
                    app.open_input("Remote url", move |app, url| {
                        let mut cli = svec(&["remote", "add"]);
                        cli.extend(args);
                        cli.push(name.clone());
                        cli.push(url);
                        app.run_git_bg(format!("add remote {name}"), cli, None);
                    });
                });
            }
            TransientAction::RemoteRename => {
                self.pick_remote("Rename remote", |app, old| {
                    app.open_input(format!("Rename {old} to"), move |app, new| {
                        app.run_git_bg(
                            format!("rename remote {old} to {new}"),
                            svec(&["remote", "rename", &old, &new]),
                            None,
                        );
                    });
                });
            }
            TransientAction::RemoteRemove => {
                self.pick_remote("Remove remote", |app, remote| {
                    app.run_git_bg(
                        format!("remove remote {remote}"),
                        svec(&["remote", "remove", &remote]),
                        None,
                    );
                });
            }
            TransientAction::RemotePrune => {
                self.pick_remote("Prune stale branches of remote", |app, remote| {
                    app.run_git_bg(
                        format!("prune remote {remote}"),
                        svec(&["remote", "prune", &remote]),
                        None,
                    );
                });
            }
            _ => unreachable!("not a remote-menu action"),
        }
    }

    /// Open a strict picker over the configured remotes.
    fn pick_remote(
        &mut self,
        prompt: impl Into<String>,
        on_submit: impl FnOnce(&mut App, String) + 'static,
    ) {
        let remotes = self.list_remotes();
        self.open_strict_picker(prompt, remotes, "no remotes", on_submit);
    }

    /// Remote names for the pickers. Listing them is a fast local read,
    /// like `list_branches`.
    pub(super) fn list_remotes(&self) -> Vec<String> {
        self.git
            .run(&["remote"])
            .map(|o| o.stdout)
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect()
    }
}
