//! A pane = one buffer: a section tree plus cursor, viewport and fold state.
//! Navigation, scrolling and refresh-survival are shared by all buffer kinds.

use crate::git::types::{DiffArea, FileDiff};
use crate::keymap::PaneKind;
use crate::ui::section::{flatten, FlatLine, Section, SectionId, SectionValue};

#[derive(Debug, Clone)]
pub struct Pane {
    pub kind: PaneKind,
    pub title: String,
    pub root: Section,
    pub flat: Vec<FlatLine>,
    /// Cursor as an index into `flat`.
    pub cursor: usize,
    /// First visible flat line.
    pub top: usize,
    /// Diffs backing the sections, looked up by (area, path) at dispatch time.
    pub unstaged: Vec<FileDiff>,
    pub staged: Vec<FileDiff>,
    pub committed: Vec<FileDiff>,
    /// For a `Log` pane: the revision args that produced it, so `g` can re-run
    /// the same log.
    pub log_args: Option<Vec<String>>,
}

/// Where the cursor was, expressed in section identities so it can be
/// restored after the tree is rebuilt.
#[derive(Debug, Clone, Default)]
struct CursorMemo {
    /// Section id chain from the cursor's section up to (not including) root.
    id_chain: Vec<SectionId>,
    was_heading: bool,
    body_idx: Option<usize>,
    flat_idx: usize,
}

impl Pane {
    pub fn new(kind: PaneKind, title: String, root: Section) -> Self {
        let flat = flatten(&root);
        Self {
            kind,
            title,
            root,
            flat,
            cursor: 0,
            top: 0,
            unstaged: Vec::new(),
            staged: Vec::new(),
            committed: Vec::new(),
            log_args: None,
        }
    }

    pub fn line_count(&self) -> usize {
        self.flat.len()
    }

    pub fn current(&self) -> Option<&FlatLine> {
        self.flat.get(self.cursor)
    }

    /// The section under the cursor (root when on header lines).
    pub fn section_at_cursor(&self) -> Option<&Section> {
        self.root.at_path(&self.current()?.path)
    }

    pub fn value_at_cursor(&self) -> SectionValue {
        self.section_at_cursor()
            .map(|s| s.value.clone())
            .unwrap_or(SectionValue::Root)
    }

    pub fn find_file(&self, area: DiffArea, path: &str) -> Option<&FileDiff> {
        let list = match area {
            DiffArea::Unstaged => &self.unstaged,
            DiffArea::Staged => &self.staged,
            DiffArea::Committed => &self.committed,
            DiffArea::Untracked => return None,
        };
        list.iter().find(|f| f.path == path)
    }

    // ---- navigation ------------------------------------------------------

    pub fn move_cursor(&mut self, delta: isize) {
        let max = self.flat.len().saturating_sub(1);
        self.cursor = self.cursor.saturating_add_signed(delta).min(max);
    }

    pub fn goto_top(&mut self) {
        self.cursor = 0;
    }

    pub fn goto_bottom(&mut self) {
        self.cursor = self.flat.len().saturating_sub(1);
    }

    /// Jump to the next visible section heading.
    pub fn next_section(&mut self) {
        if let Some(i) = (self.cursor + 1..self.flat.len()).find(|&i| self.flat[i].is_heading) {
            self.cursor = i;
        }
    }

    pub fn prev_section(&mut self) {
        if let Some(i) = (0..self.cursor).rev().find(|&i| self.flat[i].is_heading) {
            self.cursor = i;
        }
    }

    /// Jump to the heading of the parent section (or own heading when inside
    /// a section's body).
    pub fn parent_section(&mut self) {
        let Some(cur) = self.current() else { return };
        let target: Vec<usize> = if cur.is_heading {
            if cur.path.is_empty() {
                return;
            }
            cur.path[..cur.path.len() - 1].to_vec()
        } else {
            cur.path.clone()
        };
        if target.is_empty() {
            self.cursor = 0;
            return;
        }
        if let Some(i) = self
            .flat
            .iter()
            .position(|f| f.is_heading && f.path == target)
        {
            self.cursor = i;
        }
    }

    pub fn toggle_at_cursor(&mut self) {
        let Some(cur) = self.current() else { return };
        let path = cur.path.clone();
        let Some(sec) = self.root.at_path_mut(&path) else {
            return;
        };
        if !sec.is_foldable() {
            return;
        }
        sec.collapsed = !sec.collapsed;
        let id = sec.id;
        self.flat = flatten(&self.root);
        // Keep the cursor on the toggled section's heading.
        if let Some(i) = self
            .flat
            .iter()
            .position(|f| f.is_heading && f.section_id == id)
        {
            self.cursor = i;
        } else {
            self.cursor = self.cursor.min(self.flat.len().saturating_sub(1));
        }
    }

    // ---- search ----------------------------------------------------------

    /// Flat indices of lines containing `query`. Smart-case: an all-lowercase
    /// query matches case-insensitively, any uppercase makes it sensitive.
    pub fn find_matches(&self, query: &str) -> Vec<usize> {
        if query.is_empty() {
            return Vec::new();
        }
        let sensitive = query.chars().any(char::is_uppercase);
        let needle = if sensitive {
            query.to_string()
        } else {
            query.to_lowercase()
        };
        self.flat
            .iter()
            .enumerate()
            .filter(|(_, fl)| {
                let text = fl.line.to_string();
                if sensitive {
                    text.contains(&needle)
                } else {
                    text.to_lowercase().contains(&needle)
                }
            })
            .map(|(i, _)| i)
            .collect()
    }

    // ---- viewport --------------------------------------------------------

    /// Clamp the viewport so the cursor stays visible with `scrolloff` margin.
    pub fn follow(&mut self, height: usize, scrolloff: usize) {
        if height == 0 {
            return;
        }
        let margin = scrolloff.min(height.saturating_sub(1) / 2);
        let low = self
            .cursor
            .saturating_add(margin + 1)
            .saturating_sub(height);
        let high = self.cursor.saturating_sub(margin);
        self.top = self
            .top
            .clamp(low.min(high), high)
            .min(self.flat.len().saturating_sub(height.min(self.flat.len())));
    }

    // ---- refresh ---------------------------------------------------------

    /// Replace the tree, preserving fold state and cursor position by
    /// section identity (falling back to ancestors, then the raw line index).
    pub fn replace_tree(&mut self, mut root: Section) {
        let memo = self.memoize_cursor();
        root.inherit_collapse(&self.root);
        self.root = root;
        self.flat = flatten(&self.root);
        self.restore_cursor(memo);
    }

    fn memoize_cursor(&self) -> CursorMemo {
        let Some(cur) = self.current() else {
            return CursorMemo::default();
        };
        // Build the id chain from the cursor's section up through ancestors.
        let mut id_chain = Vec::new();
        let mut path = cur.path.clone();
        loop {
            if let Some(sec) = self.root.at_path(&path) {
                id_chain.push(sec.id);
            }
            if path.is_empty() {
                break;
            }
            path.pop();
        }
        CursorMemo {
            id_chain,
            was_heading: cur.is_heading,
            body_idx: cur.body_idx,
            flat_idx: self.cursor,
        }
    }

    fn restore_cursor(&mut self, memo: CursorMemo) {
        for (n, id) in memo.id_chain.iter().enumerate() {
            let exact = n == 0;
            // Prefer the same body line for an exact match, else the heading.
            if exact && !memo.was_heading {
                if let Some(i) = self.flat.iter().position(|f| {
                    f.section_id == *id && f.body_idx == memo.body_idx && !f.is_heading
                }) {
                    self.cursor = i;
                    return;
                }
            }
            if let Some(i) = self
                .flat
                .iter()
                .position(|f| f.section_id == *id && f.is_heading)
            {
                self.cursor = i;
                return;
            }
        }
        self.cursor = memo.flat_idx.min(self.flat.len().saturating_sub(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::section::Group;

    fn root_with(names: &[(&str, usize)]) -> Section {
        let mut root = Section::root();
        for (name, body_lines) in names {
            let mut s = Section::new(
                0,
                &format!("s:{name}"),
                SectionValue::Group(Group::Unstaged),
                name.to_string().into(),
            );
            for i in 0..*body_lines {
                s.body.push(format!("{name}.{i}").into());
            }
            root.children.push(s);
        }
        root
    }

    #[test]
    fn cursor_survives_refresh_by_identity() {
        // flat: A A.0 A.1 <sp> B B.0
        let mut pane = Pane::new(
            PaneKind::Status,
            "t".into(),
            root_with(&[("A", 2), ("B", 1)]),
        );
        pane.cursor = 5; // B.0
        pane.replace_tree(root_with(&[("X", 3), ("A", 2), ("B", 1)]));
        let cur = pane.current().unwrap();
        assert!(!cur.is_heading);
        assert_eq!(cur.line.to_string(), "B.0");
    }

    #[test]
    fn cursor_falls_back_to_surviving_ancestor_or_index() {
        let mut pane = Pane::new(
            PaneKind::Status,
            "t".into(),
            root_with(&[("A", 1), ("B", 1)]),
        );
        pane.cursor = 3; // spacer after A... index 3 is "B" heading? flat: A A.0 <sp> B B.0
        pane.cursor = 4; // B.0
        pane.replace_tree(root_with(&[("A", 1)]));
        // "B" vanished; cursor clamps to a valid line.
        assert!(pane.cursor < pane.line_count());
    }

    #[test]
    fn follow_keeps_cursor_within_margin() {
        let mut pane = Pane::new(PaneKind::Status, "t".into(), root_with(&[("A", 30)]));
        pane.cursor = 25;
        pane.follow(10, 3);
        assert!(pane.top <= 25 - 3 && 25 < pane.top + 10);
        pane.cursor = 2;
        pane.follow(10, 3);
        assert_eq!(pane.top, 0);
    }

    #[test]
    fn toggle_moves_cursor_to_heading_and_back() {
        let mut pane = Pane::new(PaneKind::Status, "t".into(), root_with(&[("A", 3)]));
        pane.cursor = 2; // A.1
        pane.toggle_at_cursor();
        assert_eq!(pane.line_count(), 1);
        assert!(pane.current().unwrap().is_heading);
        pane.toggle_at_cursor();
        assert_eq!(pane.line_count(), 4);
    }

    #[test]
    fn find_matches_is_smart_case() {
        let pane = Pane::new(PaneKind::Status, "t".into(), root_with(&[("Alpha", 2)]));
        // flat: "Alpha" "Alpha.0" "Alpha.1"
        assert_eq!(pane.find_matches("alpha"), vec![0, 1, 2]); // insensitive
        assert_eq!(pane.find_matches("Alpha"), vec![0, 1, 2]); // sensitive, matches
        assert_eq!(pane.find_matches("ALPHA"), Vec::<usize>::new()); // sensitive, no match
        assert_eq!(pane.find_matches(".1"), vec![2]);
        assert_eq!(pane.find_matches(""), Vec::<usize>::new());
    }

    #[test]
    fn section_navigation() {
        let mut pane = Pane::new(
            PaneKind::Status,
            "t".into(),
            root_with(&[("A", 2), ("B", 0)]),
        );
        // flat: A A.0 A.1 <sp> B
        pane.next_section();
        assert_eq!(pane.current().unwrap().line.to_string(), "B");
        pane.prev_section();
        assert_eq!(pane.current().unwrap().line.to_string(), "A");
        pane.cursor = 2; // A.1
        pane.parent_section();
        assert_eq!(pane.current().unwrap().line.to_string(), "A");
    }
}
