//! The push / pull / fetch transients (pickers resolve the target, the
//! mutation runs on a worker thread) and the remote menu
//! (add/rename/remove/prune, chaining minibuffer inputs). Push-remote and
//! upstream come from the `pushRemote`/`pushDefault` and
//! `branch.<name>.remote`/`.merge` config lookups.

use crate::app::{svec, App};
use crate::ui::transient::TransientAction;

impl App {
    pub(super) fn remote_action(&mut self, action: TransientAction, args: Vec<String>) {
        use TransientAction::*;
        // The current-branch actions need one; bail with a message early.
        let need_branch = |app: &mut App| -> Option<String> {
            let b = app.current_branch();
            if b.is_none() {
                app.message = Some("not on a branch".into());
            }
            b
        };
        match action {
            PushToPushRemote => {
                let Some(branch) = need_branch(self) else {
                    return;
                };
                let Some(remote) = self.push_remote(&branch) else {
                    self.message = Some(
                        "no push-remote (set branch.<name>.pushRemote or remote.pushDefault)"
                            .into(),
                    );
                    return;
                };
                let mut cli = svec(&["push", "-v"]);
                cli.extend(args);
                cli.push(remote.clone());
                cli.push(branch.clone());
                self.run_git_bg(format!("push {branch} to {remote}"), cli, None);
            }
            PushToUpstream => {
                let Some(branch) = need_branch(self) else {
                    return;
                };
                let Some((remote, merge)) = self.upstream_of(&branch) else {
                    self.message = Some("no upstream configured".into());
                    return;
                };
                let mut cli = svec(&["push", "-v"]);
                cli.extend(args);
                cli.push(remote.clone());
                cli.push(format!("{branch}:{merge}"));
                let short = merge.strip_prefix("refs/heads/").unwrap_or(&merge);
                self.run_git_bg(format!("push {branch} to {remote}/{short}"), cli, None);
            }
            PushElsewhere => {
                let Some(branch) = need_branch(self) else {
                    return;
                };
                let targets = self.list_remote_branches();
                self.open_picker(format!("Push {branch} to"), targets, move |app, target| {
                    app.push_branch_to(branch, target, args)
                });
            }
            PushOther => {
                let branches = self.list_branches();
                self.open_strict_picker(
                    "Push branch",
                    branches,
                    "no branches",
                    move |app, branch| {
                        let targets = app.list_remote_branches();
                        app.open_picker(
                            format!("Push {branch} to"),
                            targets,
                            move |app, target| app.push_branch_to(branch, target, args),
                        );
                    },
                );
            }
            PushRefspecs => {
                self.pick_remote("Push to remote", move |app, remote| {
                    app.open_input("Push refspecs (e.g. src:dst)", move |app, spec| {
                        let mut cli = svec(&["push", "-v"]);
                        cli.extend(args);
                        cli.push(remote.clone());
                        cli.extend(spec.split_whitespace().map(str::to_string));
                        app.run_git_bg(format!("push {spec} to {remote}"), cli, None);
                    });
                });
            }
            PushMatching => {
                self.pick_remote("Push matching branches to", move |app, remote| {
                    let mut cli = svec(&["push", "-v"]);
                    cli.extend(args);
                    cli.push(remote.clone());
                    cli.push(":".into());
                    app.run_git_bg(format!("push matching branches to {remote}"), cli, None);
                });
            }
            PushTag => {
                let tags = self.list_tags();
                self.open_strict_picker("Push tag", tags, "no tags", move |app, tag| {
                    app.pick_remote(format!("Push tag {tag} to"), move |app, remote| {
                        let mut cli = svec(&["push", "-v"]);
                        cli.extend(args);
                        cli.push(remote.clone());
                        cli.push(tag.clone());
                        app.run_git_bg(format!("push tag {tag} to {remote}"), cli, None);
                    });
                });
            }
            PushTags => {
                self.pick_remote("Push all tags to", move |app, remote| {
                    let mut cli = svec(&["push", "-v", "--tags"]);
                    cli.extend(args);
                    cli.push(remote.clone());
                    app.run_git_bg(format!("push all tags to {remote}"), cli, None);
                });
            }
            PullFromPushRemote => {
                let Some(branch) = need_branch(self) else {
                    return;
                };
                let Some(remote) = self.push_remote(&branch) else {
                    self.message = Some(
                        "no push-remote (set branch.<name>.pushRemote or remote.pushDefault)"
                            .into(),
                    );
                    return;
                };
                let mut cli = svec(&["pull"]);
                cli.extend(args);
                cli.push(remote.clone());
                cli.push(branch.clone());
                self.run_git_bg(format!("pull {remote}/{branch}"), cli, None);
            }
            PullFromUpstream => {
                let Some(branch) = need_branch(self) else {
                    return;
                };
                let Some((remote, merge)) = self.upstream_of(&branch) else {
                    self.message = Some("no upstream configured".into());
                    return;
                };
                let short = merge
                    .strip_prefix("refs/heads/")
                    .unwrap_or(&merge)
                    .to_string();
                let mut cli = svec(&["pull"]);
                cli.extend(args);
                cli.push(remote.clone());
                cli.push(short.clone());
                self.run_git_bg(format!("pull {remote}/{short}"), cli, None);
            }
            PullElsewhere => {
                let targets = self.list_remote_branches();
                self.open_picker("Pull from", targets, move |app, target| {
                    let Some((remote, target)) = app.split_remote_branch(&target) else {
                        app.message = Some("expected remote/branch".into());
                        return;
                    };
                    let mut cli = svec(&["pull"]);
                    cli.extend(args);
                    cli.push(remote.clone());
                    cli.push(target.clone());
                    app.run_git_bg(format!("pull {remote}/{target}"), cli, None);
                });
            }
            FetchFromPushRemote => {
                let Some(branch) = need_branch(self) else {
                    return;
                };
                let Some(remote) = self.push_remote(&branch) else {
                    self.message = Some(
                        "no push-remote (set branch.<name>.pushRemote or remote.pushDefault)"
                            .into(),
                    );
                    return;
                };
                let mut cli = svec(&["fetch"]);
                cli.extend(args);
                cli.push(remote.clone());
                self.run_git_bg(format!("fetch {remote}"), cli, None);
            }
            FetchFromUpstream => {
                let Some(branch) = need_branch(self) else {
                    return;
                };
                let Some((remote, _)) = self.upstream_of(&branch) else {
                    self.message = Some("no upstream configured".into());
                    return;
                };
                let mut cli = svec(&["fetch"]);
                cli.extend(args);
                cli.push(remote.clone());
                self.run_git_bg(format!("fetch {remote}"), cli, None);
            }
            FetchElsewhere => {
                self.pick_remote("Fetch from remote", move |app, remote| {
                    let mut cli = svec(&["fetch"]);
                    cli.extend(args);
                    cli.push(remote.clone());
                    app.run_git_bg(format!("fetch {remote}"), cli, None);
                });
            }
            FetchAll => {
                let mut cli = svec(&["fetch", "--all"]);
                cli.extend(args);
                self.run_git_bg("fetch --all".into(), cli, None);
            }
            FetchBranch => {
                let targets = self.list_remote_branches();
                self.open_picker("Fetch branch", targets, move |app, target| {
                    let Some((remote, target)) = app.split_remote_branch(&target) else {
                        app.message = Some("expected remote/branch".into());
                        return;
                    };
                    let mut cli = svec(&["fetch"]);
                    cli.extend(args);
                    cli.push(remote.clone());
                    cli.push(target.clone());
                    app.run_git_bg(format!("fetch {remote} {target}"), cli, None);
                });
            }
            FetchRefspec => {
                self.pick_remote("Fetch from remote", move |app, remote| {
                    app.open_input("Fetch refspec", move |app, spec| {
                        let mut cli = svec(&["fetch"]);
                        cli.extend(args);
                        cli.push(remote.clone());
                        cli.push(spec.clone());
                        app.run_git_bg(format!("fetch {spec} from {remote}"), cli, None);
                    });
                });
            }
            _ => unreachable!("not a remote action"),
        }
    }

    /// Push `branch` to a picked "remote/branch" target with the transient's
    /// flags.
    fn push_branch_to(&mut self, branch: String, target: String, flags: Vec<String>) {
        let Some((remote, target)) = self.split_remote_branch(&target) else {
            self.message = Some("expected remote/branch".into());
            return;
        };
        let mut cli = svec(&["push", "-v"]);
        cli.extend(flags);
        cli.push(remote.clone());
        cli.push(format!("{branch}:refs/heads/{target}"));
        self.run_git_bg(format!("push {branch} to {remote}/{target}"), cli, None);
    }

    /// The current branch name, per the latest status snapshot.
    pub(super) fn current_branch(&self) -> Option<String> {
        self.snapshot.as_ref().and_then(|s| s.branch.head.clone())
    }

    /// The remote pushed to by default (the "push-remote"):
    /// `branch.<name>.pushRemote`, falling back to `remote.pushDefault`.
    fn push_remote(&self, branch: &str) -> Option<String> {
        self.probe(&["config", "--get", &format!("branch.{branch}.pushRemote")])
            .or_else(|| self.probe(&["config", "--get", "remote.pushDefault"]))
    }

    /// The upstream as (remote, merge ref), e.g. ("origin", "refs/heads/main").
    fn upstream_of(&self, branch: &str) -> Option<(String, String)> {
        let remote = self.probe(&["config", "--get", &format!("branch.{branch}.remote")])?;
        let merge = self.probe(&["config", "--get", &format!("branch.{branch}.merge")])?;
        Some((remote, merge))
    }

    /// Split "remote/branch" using the actual remote names, so remotes
    /// containing slashes resolve correctly.
    fn split_remote_branch(&self, value: &str) -> Option<(String, String)> {
        for remote in self.list_remotes() {
            if let Some(rest) = value.strip_prefix(&format!("{remote}/")) {
                if !rest.is_empty() {
                    return Some((remote, rest.to_string()));
                }
            }
        }
        value
            .split_once('/')
            .map(|(r, b)| (r.to_string(), b.to_string()))
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

    /// Remote-tracking branch names ("remote/branch") for the transfer
    /// pickers.
    fn list_remote_branches(&self) -> Vec<String> {
        self.git
            .run(&["branch", "--remotes", "--format=%(refname:short)"])
            .map(|o| o.stdout)
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.ends_with("/HEAD"))
            .map(str::to_string)
            .collect()
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
