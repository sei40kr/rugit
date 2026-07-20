//! Key-sequence trie with Emacs-style key notation ("C-d", "P p", "S-TAB").
//! Lookup layering (transient > buffer-local > global) lives in `Keymaps`.

use std::collections::HashMap;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::command::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyPress {
    pub code: KeyCode,
    pub mods: KeyModifiers,
}

impl KeyPress {
    pub const fn new(code: KeyCode, mods: KeyModifiers) -> Self {
        Self { code, mods }
    }

    pub fn is_esc(&self) -> bool {
        self.code == KeyCode::Esc
    }
}

/// Canonicalize a terminal key event so that lookup is stable across
/// terminals: SHIFT is implied by the character itself, Ctrl chars are
/// lowercased, and Shift-Tab becomes `BackTab`.
pub fn normalize(ev: &KeyEvent) -> KeyPress {
    let mut mods = ev.modifiers & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT);
    let code = match ev.code {
        KeyCode::Char(c) => {
            mods.remove(KeyModifiers::SHIFT);
            if mods.contains(KeyModifiers::CONTROL) {
                KeyCode::Char(c.to_ascii_lowercase())
            } else {
                KeyCode::Char(c)
            }
        }
        KeyCode::BackTab => {
            mods.remove(KeyModifiers::SHIFT);
            KeyCode::BackTab
        }
        other => other,
    };
    KeyPress::new(code, mods)
}

/// Parse `"C-x u"`-style notation into a key sequence.
pub fn parse_keys(spec: &str) -> Result<Vec<KeyPress>, String> {
    spec.split_whitespace().map(parse_one).collect()
}

fn parse_one(tok: &str) -> Result<KeyPress, String> {
    let mut mods = KeyModifiers::NONE;
    let mut rest = tok;
    loop {
        if let Some(r) = rest.strip_prefix("C-") {
            mods |= KeyModifiers::CONTROL;
            rest = r;
        } else if let Some(r) = rest.strip_prefix("M-") {
            mods |= KeyModifiers::ALT;
            rest = r;
        } else if let Some(r) = rest.strip_prefix("S-") {
            mods |= KeyModifiers::SHIFT;
            rest = r;
        } else {
            break;
        }
        if rest.is_empty() {
            // The token was something like "C--" cut short; treat the final
            // dash as the key itself below via the empty check.
            return Err(format!("invalid key: {tok:?}"));
        }
    }
    let code = match rest {
        "TAB" => {
            if mods.contains(KeyModifiers::SHIFT) {
                mods.remove(KeyModifiers::SHIFT);
                KeyCode::BackTab
            } else {
                KeyCode::Tab
            }
        }
        "RET" | "ENTER" => KeyCode::Enter,
        "SPC" | "SPACE" => KeyCode::Char(' '),
        "ESC" => KeyCode::Esc,
        "BACKSPACE" => KeyCode::Backspace,
        "DEL" | "DELETE" => KeyCode::Delete,
        "UP" => KeyCode::Up,
        "DOWN" => KeyCode::Down,
        "LEFT" => KeyCode::Left,
        "RIGHT" => KeyCode::Right,
        "HOME" => KeyCode::Home,
        "END" => KeyCode::End,
        "PGUP" => KeyCode::PageUp,
        "PGDN" => KeyCode::PageDown,
        s => {
            let mut chars = s.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => KeyCode::Char(c),
                _ => return Err(format!("invalid key: {tok:?}")),
            }
        }
    };
    Ok(KeyPress::new(code, mods))
}

/// Render a key press back into notation (for help and the pending display).
pub fn format_key(kp: &KeyPress) -> String {
    let mut s = String::new();
    if kp.mods.contains(KeyModifiers::CONTROL) {
        s.push_str("C-");
    }
    if kp.mods.contains(KeyModifiers::ALT) {
        s.push_str("M-");
    }
    if kp.mods.contains(KeyModifiers::SHIFT) {
        s.push_str("S-");
    }
    match kp.code {
        KeyCode::Char(' ') => s.push_str("SPC"),
        KeyCode::Char(c) => s.push(c),
        KeyCode::Tab => s.push_str("TAB"),
        KeyCode::BackTab => s.push_str("S-TAB"),
        KeyCode::Enter => s.push_str("RET"),
        KeyCode::Esc => s.push_str("ESC"),
        KeyCode::Backspace => s.push_str("BACKSPACE"),
        KeyCode::Delete => s.push_str("DEL"),
        KeyCode::Up => s.push_str("UP"),
        KeyCode::Down => s.push_str("DOWN"),
        KeyCode::Left => s.push_str("LEFT"),
        KeyCode::Right => s.push_str("RIGHT"),
        KeyCode::Home => s.push_str("HOME"),
        KeyCode::End => s.push_str("END"),
        KeyCode::PageUp => s.push_str("PGUP"),
        KeyCode::PageDown => s.push_str("PGDN"),
        other => s.push_str(&format!("{other:?}")),
    }
    s
}

pub fn format_keys(seq: &[KeyPress]) -> String {
    seq.iter().map(format_key).collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Clone)]
enum Entry {
    Command(Command),
    Prefix(Keymap),
}

#[derive(Debug, Clone, Default)]
pub struct Keymap {
    map: HashMap<KeyPress, Entry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lookup {
    Command(Command),
    /// The sequence so far is a strict prefix of at least one binding.
    Pending,
    Unbound,
}

impl Keymap {
    /// Bind a sequence, replacing any conflicting binding underneath it.
    pub fn bind(&mut self, spec: &str, cmd: Command) {
        let seq = parse_keys(spec).unwrap_or_else(|e| panic!("bad default binding: {e}"));
        self.insert(&seq, cmd);
    }

    pub fn insert(&mut self, seq: &[KeyPress], cmd: Command) {
        match seq {
            [] => {}
            [k] => {
                self.map.insert(*k, Entry::Command(cmd));
            }
            [k, rest @ ..] => {
                let entry = self
                    .map
                    .entry(*k)
                    .and_modify(|e| {
                        if matches!(e, Entry::Command(_)) {
                            *e = Entry::Prefix(Keymap::default());
                        }
                    })
                    .or_insert_with(|| Entry::Prefix(Keymap::default()));
                if let Entry::Prefix(sub) = entry {
                    sub.insert(rest, cmd);
                }
            }
        }
    }

    pub fn lookup(&self, seq: &[KeyPress]) -> Lookup {
        match seq {
            [] => Lookup::Pending,
            [k, rest @ ..] => match self.map.get(k) {
                Some(Entry::Command(c)) if rest.is_empty() => Lookup::Command(*c),
                Some(Entry::Prefix(sub)) => sub.lookup(rest),
                _ => Lookup::Unbound,
            },
        }
    }

    /// Continuations available after `prefix` — feeds the which-key popup.
    pub fn candidates(&self, prefix: &[KeyPress]) -> Vec<(String, String)> {
        let mut node = self;
        for k in prefix {
            match node.map.get(k) {
                Some(Entry::Prefix(sub)) => node = sub,
                _ => return Vec::new(),
            }
        }
        let mut out: Vec<(String, String)> = node
            .map
            .iter()
            .map(|(k, e)| {
                let label = match e {
                    Entry::Command(c) => crate::command::info(*c).name.to_string(),
                    Entry::Prefix(_) => "+prefix".to_string(),
                };
                (format_key(k), label)
            })
            .collect();
        out.sort();
        out
    }

    /// All (sequence, command) pairs — feeds the help buffer.
    pub fn bindings(&self) -> Vec<(String, Command)> {
        let mut out = Vec::new();
        self.collect(&mut Vec::new(), &mut out);
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    fn collect(&self, prefix: &mut Vec<KeyPress>, out: &mut Vec<(String, Command)>) {
        for (k, e) in &self.map {
            prefix.push(*k);
            match e {
                Entry::Command(c) => out.push((format_keys(prefix), *c)),
                Entry::Prefix(sub) => sub.collect(prefix, out),
            }
            prefix.pop();
        }
    }
}

/// Buffer kinds, used to pick the buffer-local keymap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaneKind {
    Status,
    Revision,
    Log,
    ProcessLog,
    /// Branches, remote-tracking branches and tags (Magit's `magit-show-refs`).
    Refs,
    /// The interactive-rebase todo editor (Magit's git-rebase-mode).
    RebaseTodo,
}

/// Layered keymaps: buffer-local shadows global.
#[derive(Debug, Clone, Default)]
pub struct Keymaps {
    pub global: Keymap,
    pub local: HashMap<PaneKind, Keymap>,
}

impl Keymaps {
    pub fn lookup(&self, kind: PaneKind, seq: &[KeyPress]) -> Lookup {
        let local = self
            .local
            .get(&kind)
            .map(|m| m.lookup(seq))
            .unwrap_or(Lookup::Unbound);
        match (local, self.global.lookup(seq)) {
            (Lookup::Command(c), _) => Lookup::Command(c),
            (Lookup::Pending, _) | (_, Lookup::Pending) => Lookup::Pending,
            (_, g) => g,
        }
    }

    pub fn candidates(&self, kind: PaneKind, prefix: &[KeyPress]) -> Vec<(String, String)> {
        let mut out = self
            .local
            .get(&kind)
            .map(|m| m.candidates(prefix))
            .unwrap_or_default();
        for cand in self.global.candidates(prefix) {
            if !out.iter().any(|(k, _)| *k == cand.0) {
                out.push(cand);
            }
        }
        out.sort();
        out
    }
}

/// The built-in bindings. User config is merged on top of these.
pub fn default_keymaps() -> Keymaps {
    use crate::command::NavCmd::*;
    use crate::command::{Menu, TodoCmd};
    use Command::*;
    // The mnemonic keys are adapted to vim-style bindings: j/k/n/N,
    // g and z prefixes, and V (line selection) keep their vim roles, so
    // revert sits on "_", discard on "x", reset on "O" and stash on "Z".
    let mut global = Keymap::default();
    for (spec, cmd) in [
        ("q", Quit),
        ("g r", Refresh),
        ("j", Nav(MoveDown)),
        ("DOWN", Nav(MoveDown)),
        ("k", Nav(MoveUp)),
        ("UP", Nav(MoveUp)),
        ("C-d", Nav(HalfPageDown)),
        ("C-u", Nav(HalfPageUp)),
        ("C-f", Nav(PageDown)),
        ("C-b", Nav(PageUp)),
        ("PGDN", Nav(HalfPageDown)),
        ("PGUP", Nav(HalfPageUp)),
        ("HOME", Nav(GotoTop)),
        ("g g", Nav(GotoTop)),
        ("END", Nav(GotoBottom)),
        ("G", Nav(GotoBottom)),
        ("C-j", Nav(NextSection)),
        ("g j", Nav(NextSection)),
        ("C-k", Nav(PrevSection)),
        ("g k", Nav(PrevSection)),
        ("g h", Nav(ParentSection)),
        ("^", Nav(ParentSection)),
        ("TAB", ToggleSection),
        ("z a", ToggleSection),
        ("RET", Visit),
        ("/", Search),
        ("n", SearchNext),
        ("N", SearchPrev),
        ("c", Transient(Menu::Commit)),
        ("b", Transient(Menu::Branch)),
        ("m", Transient(Menu::Merge)),
        ("r", Transient(Menu::Rebase)),
        ("A", Transient(Menu::CherryPick)),
        ("_", Transient(Menu::Revert)),
        ("O", Transient(Menu::Reset)),
        ("Z", Transient(Menu::Stash)),
        ("t", Transient(Menu::Tag)),
        ("M", Transient(Menu::Remote)),
        ("p", Transient(Menu::Push)),
        ("P", Transient(Menu::Push)),
        ("F", Transient(Menu::Pull)),
        ("f", Transient(Menu::Fetch)),
        ("l", Transient(Menu::Log)),
        ("o", Transient(Menu::Submodule)),
        ("W", Transient(Menu::Worktree)),
        ("y s", Copy),
        ("y r", ShowRefs),
        ("y b", CopyRevision),
        ("?", Help),
        ("`", ProcessLog),
    ] {
        global.bind(spec, cmd);
    }

    let mut status = Keymap::default();
    for (spec, cmd) in [
        ("s", Stage),
        ("u", Unstage),
        ("S", StageAll),
        ("U", UnstageAll),
        ("x", Discard),
    ] {
        status.bind(spec, cmd);
    }

    // git-rebase-mode-style keys, adapted to vim-ish movement: j/k stay
    // navigation, so drop is on "d" (not Magit's "k") and commits move with
    // M-j/M-k.
    let mut todo = Keymap::default();
    for (spec, cmd) in [
        ("p", Todo(TodoCmd::Pick)),
        ("r", Todo(TodoCmd::Reword)),
        ("e", Todo(TodoCmd::Edit)),
        ("s", Todo(TodoCmd::Squash)),
        ("f", Todo(TodoCmd::Fixup)),
        ("d", Todo(TodoCmd::Drop)),
        ("M-k", Todo(TodoCmd::MoveUp)),
        ("M-j", Todo(TodoCmd::MoveDown)),
        ("C-c C-c", Todo(TodoCmd::Confirm)),
        ("C-c C-k", Todo(TodoCmd::Abort)),
    ] {
        todo.bind(spec, cmd);
    }

    let mut local = HashMap::new();
    local.insert(PaneKind::Status, status);
    local.insert(PaneKind::RebaseTodo, todo);
    Keymaps { global, local }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Menu;

    #[test]
    fn parse_and_format_roundtrip() {
        for spec in ["C-d", "P p", "S-TAB", "RET", "M-x", "SPC", "$", "?"] {
            let seq = parse_keys(spec).unwrap();
            assert_eq!(format_keys(&seq), spec, "roundtrip of {spec:?}");
        }
    }

    #[test]
    fn sequence_lookup() {
        let mut km = Keymap::default();
        km.bind("P p", Command::Transient(Menu::Push));
        km.bind("g", Command::Refresh);
        let p = parse_keys("P").unwrap();
        assert_eq!(km.lookup(&p), Lookup::Pending);
        let pp = parse_keys("P p").unwrap();
        assert_eq!(
            km.lookup(&pp),
            Lookup::Command(Command::Transient(Menu::Push))
        );
        let px = parse_keys("P x").unwrap();
        assert_eq!(km.lookup(&px), Lookup::Unbound);
        assert_eq!(
            km.candidates(&p),
            vec![("p".to_string(), "push".to_string())]
        );
    }

    #[test]
    fn rebinding_a_prefix_replaces_it() {
        let mut km = Keymap::default();
        km.bind("P", Command::Refresh);
        km.bind("P p", Command::Transient(Menu::Push));
        assert_eq!(
            km.lookup(&parse_keys("P p").unwrap()),
            Lookup::Command(Command::Transient(Menu::Push))
        );
    }

    #[test]
    fn local_shadows_global() {
        let kms = default_keymaps();
        let s = parse_keys("s").unwrap();
        assert_eq!(
            kms.lookup(PaneKind::Status, &s),
            Lookup::Command(Command::Stage)
        );
        assert_eq!(kms.lookup(PaneKind::Revision, &s), Lookup::Unbound);
    }

    #[test]
    fn rebase_todo_keys_shadow_global_only_in_that_buffer() {
        use crate::command::TodoCmd;
        let kms = default_keymaps();
        let d = parse_keys("d").unwrap();
        assert_eq!(
            kms.lookup(PaneKind::RebaseTodo, &d),
            Lookup::Command(Command::Todo(TodoCmd::Drop))
        );
        assert_eq!(kms.lookup(PaneKind::Status, &d), Lookup::Unbound);
        // The two-key confirm sequence resolves through the trie.
        let confirm = parse_keys("C-c C-c").unwrap();
        assert_eq!(
            kms.lookup(PaneKind::RebaseTodo, &confirm),
            Lookup::Command(Command::Todo(TodoCmd::Confirm))
        );
        // j/k stay navigation; commits move with M-j/M-k.
        assert_eq!(
            kms.lookup(PaneKind::RebaseTodo, &parse_keys("k").unwrap()),
            Lookup::Command(Command::Nav(crate::command::NavCmd::MoveUp))
        );
        assert_eq!(
            kms.lookup(PaneKind::RebaseTodo, &parse_keys("M-k").unwrap()),
            Lookup::Command(Command::Todo(TodoCmd::MoveUp))
        );
    }

    #[test]
    fn normalize_strips_shift_from_chars() {
        let ev = KeyEvent::new(KeyCode::Char('P'), KeyModifiers::SHIFT);
        let kp = normalize(&ev);
        assert_eq!(kp, KeyPress::new(KeyCode::Char('P'), KeyModifiers::NONE));
    }
}
