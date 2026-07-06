//! The branch transient: checkout via pickers, creation
//! chaining name → starting point, spin-off/out moving the unpushed
//! commits onto a new branch, plus rename/reset/delete. Reset of the
//! current branch and deletion of an unmerged branch confirm first, since
//! work would be lost.

use std::thread;

use crate::app::workers::refresh_index_stat_cache;
use crate::app::{svec, App, AppEvent, Confirm, PendingAction};
use crate::git::client::{display_cmd, GitClient, ProcessEntry};
use crate::ui::section::SectionValue;
use crate::ui::transient::TransientAction;

impl App {
    pub(super) fn branch_action(&mut self, action: TransientAction) {
        match action {
            TransientAction::Checkout => {
                let revs = self.list_revs_at_point();
                self.open_picker("Checkout", revs, |app, rev| app.checkout_rev(rev));
            }
            TransientAction::CheckoutLocal => {
                let branches = self.list_local_branches();
                self.open_strict_picker(
                    "Checkout local branch",
                    branches,
                    "no local branches",
                    |app, rev| app.checkout_rev(rev),
                );
            }
            TransientAction::CreateCheckoutBranch => self.create_branch(true),
            TransientAction::CreateBranch => self.create_branch(false),
            TransientAction::BranchSpinoff => {
                self.open_input("Spin off branch", |app, name| {
                    app.spinoff_branch(name, true)
                });
            }
            TransientAction::BranchSpinout => {
                self.open_input("Spin out branch", |app, name| {
                    app.spinoff_branch(name, false)
                });
            }
            TransientAction::BranchRename => {
                let branches = self.list_local_branches();
                self.open_strict_picker(
                    "Rename branch",
                    branches,
                    "no local branches",
                    |app, old| {
                        app.open_input(format!("Rename branch '{old}' to"), move |app, new| {
                            app.run_git_bg(
                                format!("rename branch {old} to {new}"),
                                svec(&["branch", "--move", &old, &new]),
                                None,
                            );
                        });
                    },
                );
            }
            TransientAction::BranchReset => {
                let branches = self.list_local_branches();
                self.open_strict_picker(
                    "Reset branch",
                    branches,
                    "no local branches",
                    |app, branch| {
                        // Default the target to the branch's upstream.
                        let mut candidates = app.list_revs_at_point();
                        candidates.retain(|c| *c != branch);
                        if let Some(upstream) = app.probe(&[
                            "rev-parse",
                            "--abbrev-ref",
                            &format!("{branch}@{{upstream}}"),
                        ]) {
                            candidates.retain(|c| *c != upstream);
                            candidates.insert(0, upstream);
                        }
                        app.open_picker(
                            format!("Reset {branch} to"),
                            candidates,
                            move |app, rev| app.reset_branch(branch, rev),
                        );
                    },
                );
            }
            TransientAction::BranchDelete => {
                let branches = self.list_local_branches();
                self.open_strict_picker(
                    "Delete branch",
                    branches,
                    "no local branches",
                    |app, branch| app.delete_branch(branch),
                );
            }
            TransientAction::BranchConfigure => {
                let branches = self.list_local_branches();
                self.open_strict_picker(
                    "Configure branch",
                    branches,
                    "no local branches",
                    |app, branch| {
                        app.open_configure_transient(
                            &crate::ui::transient::BRANCH_CONFIGURE,
                            branch,
                        );
                    },
                );
            }
            _ => unreachable!("not a branch action"),
        }
    }

    /// `git checkout` DWIMs: local branch, remote-tracking branch (creates
    /// a local branch), tag or raw revision (detaches).
    fn checkout_rev(&mut self, rev: String) {
        self.run_git_bg(format!("checkout {rev}"), svec(&["checkout", &rev]), None);
    }

    /// Ask for the starting point, then the name; `checkout` decides
    /// between `checkout -b` and a plain `branch`.
    fn create_branch(&mut self, checkout: bool) {
        let prompt = if checkout {
            "Create and checkout branch starting at"
        } else {
            "Create branch starting at"
        };
        let starts = self.list_start_points();
        self.open_picker(prompt, starts, move |app, start| {
            app.open_input(
                format!("Name for new branch (starting at {start})"),
                move |app, name| {
                    if checkout {
                        app.run_git_bg(
                            format!("create+checkout {name}"),
                            svec(&["checkout", "-b", &name, &start]),
                            None,
                        );
                    } else {
                        app.run_git_bg(
                            format!("create branch {name}"),
                            svec(&["branch", &name, &start]),
                            None,
                        );
                    }
                },
            );
        });
    }

    fn reset_branch(&mut self, branch: String, rev: String) {
        let current = self.snapshot.as_ref().and_then(|s| s.branch.head.clone());
        if current.as_deref() == Some(&branch) {
            // Resetting the checked-out branch is a hard reset, so
            // uncommitted changes are lost.
            self.confirm = Some(Confirm {
                prompt: format!("Discard uncommitted changes and hard-reset to {rev}?"),
                action: PendingAction::Git {
                    desc: format!("reset {branch} to {rev}"),
                    args: svec(&["reset", "--hard", &rev]),
                    stdin: None,
                },
            });
        } else {
            self.run_git_bg(
                format!("reset {branch} to {rev}"),
                svec(&["branch", "--force", &branch, &rev]),
                None,
            );
        }
    }

    fn delete_branch(&mut self, branch: String) {
        // Deleting a merged branch loses nothing; an unmerged one needs
        // --force and a confirmation.
        let merged = self
            .git
            .run(&["merge-base", "--is-ancestor", &branch, "HEAD"])
            .map(|o| o.status == 0)
            .unwrap_or(false);
        if merged {
            self.run_git_bg(
                format!("delete branch {branch}"),
                svec(&["branch", "--delete", &branch]),
                None,
            );
        } else {
            self.confirm = Some(Confirm {
                prompt: format!("Delete unmerged branch {branch}?"),
                action: PendingAction::Git {
                    desc: format!("delete branch {branch}"),
                    args: svec(&["branch", "--delete", "--force", &branch]),
                    stdin: None,
                },
            });
        }
    }

    /// Create `name` at the current branch and move the unpushed commits to
    /// it: the old branch is reset to the last commit it shares with its
    /// upstream (a "spin-off"). With `checkout` HEAD moves to the
    /// new branch; without, HEAD stays and the old branch is hard-reset —
    /// only safe on a clean worktree, so a dirty one forces the checkout.
    fn spinoff_branch(&mut self, name: String, mut checkout: bool) {
        let current = self.snapshot.as_ref().and_then(|s| s.branch.head.clone());
        if !checkout {
            let dirty = self.snapshot.as_ref().is_none_or(|s| {
                !s.staged.is_empty() || !s.unstaged.is_empty() || !s.unmerged.is_empty()
            });
            if dirty {
                self.message = Some("uncommitted changes; checking out the new branch".into());
                checkout = true;
            }
        }
        let desc = format!("spin {} {name}", if checkout { "off" } else { "out" });
        self.busy = Some(desc.clone());
        let git = self.git.clone();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let mut ran = Vec::new();
            let mut output = String::new();
            let status = spinoff_steps(
                &git,
                &name,
                current.as_deref(),
                checkout,
                &mut ran,
                &mut output,
            );
            refresh_index_stat_cache(&git);
            let entry = ProcessEntry {
                cmd: ran.join(" && "),
                status,
                output,
            };
            let _ = tx.send(AppEvent::GitDone { desc, entry });
        });
    }

    /// A read-only git question: Some(trimmed stdout) on success.
    pub(super) fn probe(&self, args: &[&str]) -> Option<String> {
        self.git
            .run(args)
            .ok()
            .filter(|o| o.status == 0)
            .map(|o| o.stdout.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// Starting-point candidates for branch creation: the commit at point
    /// first, then branches.
    fn list_start_points(&self) -> Vec<String> {
        self.list_revs_at_point()
    }

    /// `list_branches`, but with the commit at point (if any) as the first
    /// candidate — merge/rebase pickers default to the thing under the
    /// cursor, like Magit's at-point defaults.
    pub(super) fn list_revs_at_point(&self) -> Vec<String> {
        let mut out = self.list_branches();
        if let Some(SectionValue::Commit { hash }) = self.panes.last().map(|p| p.value_at_cursor())
        {
            out.retain(|b| *b != hash);
            out.insert(0, hash);
        }
        out
    }

    /// Local and remote-tracking branch names for the checkout picker.
    /// Listing refs is a fast local read, so this runs synchronously.
    pub(super) fn list_branches(&self) -> Vec<String> {
        let out = self
            .git
            .run(&["branch", "--all", "--format=%(refname:short)"])
            .ok();
        let mut seen = std::collections::BTreeSet::new();
        out.map(|o| o.stdout)
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.ends_with("HEAD") && !l.contains("HEAD detached"))
            .filter(|l| seen.insert(l.to_string()))
            .map(str::to_string)
            .collect()
    }

    /// Local branch names only, for the rename/reset/delete pickers.
    fn list_local_branches(&self) -> Vec<String> {
        self.git
            .run(&["branch", "--format=%(refname:short)"])
            .map(|o| o.stdout)
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.contains("HEAD detached"))
            .map(str::to_string)
            .collect()
    }
}

/// The mutating steps of a spin-off, run on the worker thread; returns the
/// exit status for the process-log entry. Read-only probes are not logged.
fn spinoff_steps(
    git: &GitClient,
    name: &str,
    current: Option<&str>,
    checkout: bool,
    ran: &mut Vec<String>,
    output: &mut String,
) -> i32 {
    let step = |args: &[&str], output: &mut String, ran: &mut Vec<String>| -> i32 {
        ran.push(display_cmd(
            &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        ));
        match git.run(args) {
            Ok(o) => {
                output.push_str(&o.stdout);
                output.push_str(&o.stderr);
                o.status
            }
            Err(e) => {
                output.push_str(&e.to_string());
                -1
            }
        }
    };
    let probe = |args: &[&str]| -> Option<String> {
        git.run(args)
            .ok()
            .filter(|o| o.status == 0)
            .map(|o| o.stdout.trim().to_string())
            .filter(|s| !s.is_empty())
    };

    // Detached HEAD: nothing to reset, just create the branch here.
    let Some(current) = current else {
        let args: &[&str] = if checkout {
            &["checkout", "-b", name]
        } else {
            &["branch", name]
        };
        return step(args, output, ran);
    };

    let status = if checkout {
        step(&["checkout", "-b", name, current], output, ran)
    } else {
        step(&["branch", name, current], output, ran)
    };
    if status != 0 {
        return status;
    }

    // Reset the old branch to the last commit it shares with its upstream;
    // without an upstream (or unpushed commits) it stays put.
    let Some(upstream) = probe(&[
        "rev-parse",
        "--abbrev-ref",
        &format!("{current}@{{upstream}}"),
    ]) else {
        return 0;
    };
    let Some(base) = probe(&["merge-base", current, &upstream]) else {
        return 0;
    };
    if probe(&["rev-parse", current]).as_deref() == Some(&base) {
        return 0;
    }
    if checkout {
        step(
            &[
                "update-ref",
                "-m",
                &format!("reset: moving to {base}"),
                &format!("refs/heads/{current}"),
                &base,
            ],
            output,
            ran,
        )
    } else {
        step(&["reset", "--hard", &base], output, ran)
    }
}
