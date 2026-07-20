//! The submodule transient. Add/register/populate/update/synchronize run on
//! all submodules (git's default when no path is given); unpopulate and
//! remove pick a specific submodule, and remove confirms first since it
//! deletes the working tree. Each subcommand only receives the switches git
//! accepts on it.

use crate::app::{svec, App, Confirm, PendingAction};
use crate::ui::transient::TransientAction;

impl App {
    pub(super) fn submodule_action(&mut self, action: TransientAction, args: Vec<String>) {
        use TransientAction::*;
        let has = |f: &str| args.iter().any(|a| a == f);
        match action {
            SubmoduleAdd => {
                // Ask for the URL, then an optional path; `--force` rides along.
                let force = has("--force");
                self.open_input("Submodule url", move |app, url| {
                    app.open_input("Submodule path (blank for default)", move |app, path| {
                        let mut cli = svec(&["submodule", "add"]);
                        if force {
                            cli.push("--force".into());
                        }
                        cli.push(url.clone());
                        if !path.is_empty() {
                            cli.push(path);
                        }
                        app.run_git_bg(format!("add submodule {url}"), cli, None);
                    });
                });
            }
            SubmoduleRegister => {
                self.run_git_bg(
                    "register submodules".into(),
                    svec(&["submodule", "init"]),
                    None,
                );
            }
            SubmodulePopulate => {
                let mut cli = svec(&["submodule", "update", "--init"]);
                if has("--recursive") {
                    cli.push("--recursive".into());
                }
                if has("--force") {
                    cli.push("--force".into());
                }
                self.run_git_bg("populate submodules".into(), cli, None);
            }
            SubmoduleUpdate => {
                let mut cli = svec(&["submodule", "update"]);
                for flag in ["--recursive", "--no-fetch", "--force"] {
                    if has(flag) {
                        cli.push(flag.into());
                    }
                }
                self.run_git_bg("update submodules".into(), cli, None);
            }
            SubmoduleSync => {
                let mut cli = svec(&["submodule", "sync"]);
                if has("--recursive") {
                    cli.push("--recursive".into());
                }
                self.run_git_bg("sync submodules".into(), cli, None);
            }
            SubmoduleUnpopulate => {
                let force = has("--force");
                let subs = self.list_submodules();
                self.open_strict_picker(
                    "Unpopulate submodule",
                    subs,
                    "no submodules",
                    move |app, path| {
                        let mut cli = svec(&["submodule", "deinit"]);
                        if force {
                            cli.push("--force".into());
                        }
                        cli.push(path.clone());
                        app.run_git_bg(format!("deinit submodule {path}"), cli, None);
                    },
                );
            }
            SubmoduleRemove => {
                let subs = self.list_submodules();
                self.open_strict_picker("Remove submodule", subs, "no submodules", |app, path| {
                    // Removing deletes the submodule's working tree, so
                    // confirm; deinit -f then `git rm -f` clears both the
                    // checkout and the .gitmodules/index entries.
                    app.confirm = Some(Confirm {
                        prompt: format!("Remove submodule {path} and its working tree?"),
                        action: PendingAction::GitSeq {
                            desc: format!("remove submodule {path}"),
                            cmds: vec![
                                svec(&["submodule", "deinit", "--force", &path]),
                                svec(&["rm", "--force", &path]),
                            ],
                        },
                    });
                });
            }
            _ => unreachable!("not a submodule action"),
        }
    }

    /// Submodule paths from `.gitmodules`, for the unpopulate/remove pickers.
    /// Reading config is a fast local operation, like `list_remotes`.
    pub(super) fn list_submodules(&self) -> Vec<String> {
        self.git
            .run(&[
                "config",
                "--file",
                ".gitmodules",
                "--get-regexp",
                r"^submodule\..*\.path$",
            ])
            .map(|o| o.stdout)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.split_once(' ').map(|(_, path)| path.trim().to_string()))
            .filter(|p| !p.is_empty())
            .collect()
    }
}
