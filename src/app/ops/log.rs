//! The log transient: each action appends its revision selector to the
//! collected options and opens a log buffer.

use crate::app::App;
use crate::ui::input::{InputPurpose, InputState};
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
                // Carry the log options through the rev picker to the submit.
                self.input = Some(
                    InputState::picker("Log", InputPurpose::LogRev, self.list_branches())
                        .with_carry(args),
                );
            }
            _ => unreachable!("not a log action"),
        }
    }

    pub(super) fn log_submit(&mut self, value: String, carry: Vec<String>) {
        // `carry` holds the log options collected in the transient.
        let mut extra = carry;
        extra.push(value.clone());
        self.load_log(format!("Commits in {value}"), extra, false);
    }
}
