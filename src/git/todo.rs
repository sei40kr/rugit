//! Pure rebase-todo logic backing the in-app todo editor (Magit's
//! git-rebase-mode): the entry model, parsing and serializing git's
//! `git-rebase-todo` format, and `--autosquash` reordering. No I/O — the
//! editor buffer lives in `ui/`, launching git lives in `app/ops/rebase.rs`.

/// The subset of todo instructions the in-app editor understands. Anything
/// else (`exec`, `break`, `merge`, ...) makes `parse_todo` bail so we fall
/// back to $EDITOR instead of silently corrupting the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoAction {
    Pick,
    Reword,
    Edit,
    Squash,
    Fixup,
    Drop,
}

impl TodoAction {
    pub fn word(self) -> &'static str {
        match self {
            TodoAction::Pick => "pick",
            TodoAction::Reword => "reword",
            TodoAction::Edit => "edit",
            TodoAction::Squash => "squash",
            TodoAction::Fixup => "fixup",
            TodoAction::Drop => "drop",
        }
    }

    /// Both the long word and git's single-letter abbreviation.
    fn from_word(w: &str) -> Option<Self> {
        Some(match w {
            "pick" | "p" => TodoAction::Pick,
            "reword" | "r" => TodoAction::Reword,
            "edit" | "e" => TodoAction::Edit,
            "squash" | "s" => TodoAction::Squash,
            "fixup" | "f" => TodoAction::Fixup,
            "drop" | "d" => TodoAction::Drop,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoEntry {
    pub action: TodoAction,
    /// Abbreviated commit hash, also the entry's identity in the editor.
    pub hash: String,
    pub subject: String,
}

/// Parse a `git-rebase-todo` file. Comments and blank lines are dropped.
/// `Err` names the first unsupported instruction, so the caller can fall
/// back to $EDITOR for plans the editor cannot represent.
pub fn parse_todo(text: &str) -> Result<Vec<TodoEntry>, String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, ' ');
        let word = parts.next().unwrap_or_default();
        let Some(action) = TodoAction::from_word(word) else {
            return Err(format!("unsupported todo instruction {word:?}"));
        };
        let Some(hash) = parts.next() else {
            return Err(format!("malformed todo line {line:?}"));
        };
        out.push(TodoEntry {
            action,
            hash: hash.to_string(),
            subject: parts.next().unwrap_or_default().to_string(),
        });
    }
    Ok(out)
}

/// Serialize entries back into the format git's sequencer reads. `drop`
/// lines are written out (rather than omitted) so the intent stays visible
/// in the file.
pub fn serialize_todo(entries: &[TodoEntry]) -> String {
    let mut out = String::new();
    for e in entries {
        out.push_str(e.action.word());
        out.push(' ');
        out.push_str(&e.hash);
        if !e.subject.is_empty() {
            out.push(' ');
            out.push_str(&e.subject);
        }
        out.push('\n');
    }
    out
}

/// The action and target-subject prefix encoded in a `fixup!`/`squash!`
/// marker subject, `None` for ordinary commits.
fn target_of(e: &TodoEntry) -> Option<(TodoAction, &str)> {
    if let Some(rest) = e.subject.strip_prefix("fixup! ") {
        Some((TodoAction::Fixup, rest))
    } else if let Some(rest) = e.subject.strip_prefix("squash! ") {
        Some((TodoAction::Squash, rest))
    } else {
        None
    }
}

/// `--autosquash` for a todo we generate ourselves: move each `fixup!`/
/// `squash!` commit directly after its target (the earliest non-marker
/// commit whose subject starts with the rest of the marker subject) and set
/// its action accordingly. Markers without a target stay where they are, as
/// plain picks — same as git.
pub fn autosquash(entries: Vec<TodoEntry>) -> Vec<TodoEntry> {
    // A marker is movable only when some non-marker entry matches it.
    let is_base = |e: &TodoEntry| target_of(e).is_none();
    let movable = |e: &TodoEntry| {
        target_of(e).is_some_and(|(_, rest)| {
            entries
                .iter()
                .any(|b| is_base(b) && b.subject.starts_with(rest))
        })
    };

    let mut out = Vec::with_capacity(entries.len());
    for e in &entries {
        if movable(e) {
            continue; // placed below, right after its target
        }
        out.push(e.clone());
        if !is_base(e) {
            continue; // an unmatched marker cannot be a target itself
        }
        for m in &entries {
            let Some((action, rest)) = target_of(m) else {
                continue;
            };
            // Attach to the *first* matching base only, like git.
            let first_match = entries
                .iter()
                .find(|b| is_base(b) && b.subject.starts_with(rest));
            if first_match.map(|b| b.hash == e.hash) == Some(true) {
                out.push(TodoEntry {
                    action,
                    ..m.clone()
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(action: TodoAction, hash: &str, subject: &str) -> TodoEntry {
        TodoEntry {
            action,
            hash: hash.into(),
            subject: subject.into(),
        }
    }

    #[test]
    fn parse_accepts_long_and_abbreviated_actions_and_skips_comments() {
        let text = "# comment\npick abc123 first\nf def456 fixup! first\n\ndrop 789abc gone\n";
        let entries = parse_todo(text).unwrap();
        assert_eq!(
            entries,
            vec![
                e(TodoAction::Pick, "abc123", "first"),
                e(TodoAction::Fixup, "def456", "fixup! first"),
                e(TodoAction::Drop, "789abc", "gone"),
            ]
        );
    }

    #[test]
    fn parse_rejects_unsupported_instructions() {
        assert!(parse_todo("exec make test\n").is_err());
        assert!(parse_todo("break\n").is_err());
    }

    #[test]
    fn serialize_roundtrips_through_parse() {
        let entries = vec![
            e(TodoAction::Reword, "abc123", "first"),
            e(TodoAction::Drop, "def456", "second"),
        ];
        let text = serialize_todo(&entries);
        assert_eq!(text, "reword abc123 first\ndrop def456 second\n");
        assert_eq!(parse_todo(&text).unwrap(), entries);
    }

    #[test]
    fn autosquash_moves_markers_after_their_target() {
        let entries = vec![
            e(TodoAction::Pick, "a", "feature"),
            e(TodoAction::Pick, "b", "other work"),
            e(TodoAction::Pick, "c", "fixup! feature"),
            e(TodoAction::Pick, "d", "squash! other work"),
        ];
        let out = autosquash(entries);
        let plan: Vec<(&str, TodoAction)> =
            out.iter().map(|x| (x.hash.as_str(), x.action)).collect();
        assert_eq!(
            plan,
            vec![
                ("a", TodoAction::Pick),
                ("c", TodoAction::Fixup),
                ("b", TodoAction::Pick),
                ("d", TodoAction::Squash),
            ]
        );
    }

    #[test]
    fn autosquash_leaves_unmatched_markers_in_place() {
        let entries = vec![
            e(TodoAction::Pick, "a", "feature"),
            e(TodoAction::Pick, "b", "fixup! no such subject"),
        ];
        assert_eq!(autosquash(entries.clone()), entries);
    }

    #[test]
    fn autosquash_stacks_multiple_fixups_in_order() {
        let entries = vec![
            e(TodoAction::Pick, "a", "feature"),
            e(TodoAction::Pick, "b", "fixup! feature"),
            e(TodoAction::Pick, "c", "fixup! feature"),
        ];
        let out = autosquash(entries);
        let hashes: Vec<&str> = out.iter().map(|x| x.hash.as_str()).collect();
        assert_eq!(hashes, vec!["a", "b", "c"]);
        assert!(out[1..].iter().all(|x| x.action == TodoAction::Fixup));
    }
}
