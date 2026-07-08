//! The log transient: each action appends its revision selector to the
//! collected options and opens a log buffer.

use crate::app::App;
use crate::ui::transient::TransientAction;

impl App {
    pub(super) fn log_action(&mut self, action: TransientAction, mut args: Vec<String>) {
        match action {
            TransientAction::LogCurrent => {
                // Name the current branch in the header (magit shows "Commits in
                // main"), falling back to HEAD when detached / unborn.
                let name = self
                    .snapshot
                    .as_ref()
                    .and_then(|s| s.branch.head.clone())
                    .unwrap_or_else(|| "HEAD".to_string());
                args.push("HEAD".to_string());
                self.load_log(format!("Commits in {name}"), args, false)
            }
            TransientAction::LogAll => {
                args.push("--all".to_string());
                self.load_log("Commits in all references".into(), args, false)
            }
            TransientAction::LogOther => {
                // The log options ride into the continuation as captures.
                let revs = self.list_branches();
                self.open_picker("Log", revs, move |app, rev| {
                    args.push(rev.clone());
                    app.load_log(format!("Commits in {rev}"), args, false);
                });
            }
            _ => unreachable!("not a log action"),
        }
    }
}
