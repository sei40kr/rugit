//! The log transient: each action appends its revision
//! selector to the collected options and opens a log buffer. Reflogs
//! reuse the same buffer via `--walk-reflogs`.

use crate::app::App;
use crate::ui::transient::TransientAction;

impl App {
    pub(super) fn log_action(&mut self, action: TransientAction, mut args: Vec<String>) {
        // Name the current branch in headers ("Commits in main"),
        // falling back to HEAD when detached / unborn.
        let head = self.current_branch().unwrap_or_else(|| "HEAD".to_string());
        match action {
            TransientAction::LogCurrent => {
                args.push("HEAD".to_string());
                self.load_log(format!("Commits in {head}"), args, false)
            }
            TransientAction::LogRelated => {
                // The current branch plus its upstream and push-remote
                // counterparts.
                args.push("HEAD".to_string());
                if let Some(upstream) = self.probe(&["rev-parse", "--abbrev-ref", "@{upstream}"]) {
                    args.push(upstream);
                }
                if let Some(push) = self.probe(&["rev-parse", "--abbrev-ref", "@{push}"]) {
                    if !args.contains(&push) {
                        args.push(push);
                    }
                }
                self.load_log(format!("Commits related to {head}"), args, false)
            }
            TransientAction::LogLocalBranches => {
                args.push("--branches".to_string());
                self.load_log("Commits in local branches".into(), args, false)
            }
            TransientAction::LogAllBranches => {
                args.push("--branches".to_string());
                args.push("--remotes".to_string());
                self.load_log("Commits in all branches".into(), args, false)
            }
            TransientAction::LogAll => {
                args.push("--all".to_string());
                self.load_log("Commits in all references".into(), args, false)
            }
            TransientAction::LogOther => {
                // The log options ride into the rev picker as captures.
                let revs = self.list_revs_at_point();
                self.open_picker("Log", revs, move |app, rev| {
                    let mut extra = args;
                    extra.push(rev.clone());
                    app.load_log(format!("Commits in {rev}"), extra, false);
                });
            }
            TransientAction::ReflogCurrent => {
                args.push("--walk-reflogs".to_string());
                args.push(head.clone());
                self.load_log(format!("Reflog for {head}"), args, false)
            }
            TransientAction::ReflogHead => {
                args.push("--walk-reflogs".to_string());
                args.push("HEAD".to_string());
                self.load_log("Reflog for HEAD".into(), args, false)
            }
            TransientAction::ReflogOther => {
                let branches = self.list_branches();
                self.open_picker("Show reflog for", branches, move |app, rev| {
                    let mut extra = args;
                    extra.push("--walk-reflogs".to_string());
                    extra.push(rev.clone());
                    app.load_log(format!("Reflog for {rev}"), extra, false);
                });
            }
            _ => unreachable!("not a log action"),
        }
    }

    /// Recent commit authors ("Name <email>") for the --author= picker.
    /// Typed input still wins when nothing matches.
    pub(super) fn list_recent_authors(&self) -> Vec<String> {
        let mut seen = std::collections::BTreeSet::new();
        self.git
            .run(&["log", "-n300", "--format=%aN <%aE>"])
            .map(|o| o.stdout)
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .filter(|l| seen.insert(l.to_string()))
            .map(str::to_string)
            .collect()
    }
}
