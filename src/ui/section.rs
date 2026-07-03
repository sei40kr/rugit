//! The section tree — Magit's core abstraction. Every buffer is a tree of
//! sections; commands act on the section at point (DWIM dispatch).

use std::hash::{Hash, Hasher};

use ratatui::text::Line;

use crate::git::types::DiffArea;

pub type SectionId = u64;

/// What the section *is* — the value commands dispatch on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SectionValue {
    Root,
    Group(Group),
    File {
        area: DiffArea,
        path: String,
    },
    Hunk {
        area: DiffArea,
        path: String,
        /// Index into the owning `FileDiff::hunks`.
        hunk_idx: usize,
    },
    Commit {
        hash: String,
    },
    Stash {
        index: usize,
    },
    /// Inert content (process log entries, revision headers, ...).
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Untracked,
    Unmerged,
    Unstaged,
    Staged,
    Stashes,
    Recent,
}

#[derive(Debug, Clone)]
pub struct Section {
    /// Stable identity across refreshes (hash of parent id + own key).
    pub id: SectionId,
    pub value: SectionValue,
    pub heading: Line<'static>,
    pub body: Vec<Line<'static>>,
    pub children: Vec<Section>,
    pub collapsed: bool,
}

impl Section {
    pub fn root() -> Self {
        Self {
            id: 0,
            value: SectionValue::Root,
            heading: Line::default(),
            body: Vec::new(),
            children: Vec::new(),
            collapsed: false,
        }
    }

    pub fn new(
        parent_id: SectionId,
        key: &str,
        value: SectionValue,
        heading: Line<'static>,
    ) -> Self {
        Self {
            id: section_id(parent_id, key),
            value,
            heading,
            body: Vec::new(),
            children: Vec::new(),
            collapsed: false,
        }
    }

    pub fn is_foldable(&self) -> bool {
        !self.children.is_empty() || !self.body.is_empty()
    }

    /// Resolve a child-index path (as stored in `FlatLine::path`).
    pub fn at_path(&self, path: &[usize]) -> Option<&Section> {
        let mut node = self;
        for &i in path {
            node = node.children.get(i)?;
        }
        Some(node)
    }

    pub fn at_path_mut(&mut self, path: &[usize]) -> Option<&mut Section> {
        let mut node = self;
        for &i in path {
            node = node.children.get_mut(i)?;
        }
        Some(node)
    }

    /// Copy collapse state from a previous tree by section identity.
    pub fn inherit_collapse(&mut self, old: &Section) {
        let mut states = Vec::new();
        collect_collapse(old, &mut states);
        apply_collapse(self, &states);
    }
}

pub fn section_id(parent: SectionId, key: &str) -> SectionId {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    parent.hash(&mut h);
    key.hash(&mut h);
    h.finish()
}

fn collect_collapse(s: &Section, out: &mut Vec<(SectionId, bool)>) {
    out.push((s.id, s.collapsed));
    for c in &s.children {
        collect_collapse(c, out);
    }
}

fn apply_collapse(s: &mut Section, states: &[(SectionId, bool)]) {
    if let Some((_, collapsed)) = states.iter().find(|(id, _)| *id == s.id) {
        s.collapsed = *collapsed;
    }
    for c in &mut s.children {
        apply_collapse(c, states);
    }
}

/// One visible line of a flattened tree. Rendering and cursor movement both
/// operate on `Vec<FlatLine>`; it is rebuilt on fold/refresh only.
#[derive(Debug, Clone)]
pub struct FlatLine {
    /// Child-index path from the root to the owning section.
    pub path: Vec<usize>,
    pub section_id: SectionId,
    pub is_heading: bool,
    /// Index into the owning section's `body` (None for headings/spacers).
    pub body_idx: Option<usize>,
    pub line: Line<'static>,
}

pub fn flatten(root: &Section) -> Vec<FlatLine> {
    let mut out = Vec::new();
    // Root body lines render first (status header block).
    for (i, l) in root.body.iter().enumerate() {
        out.push(FlatLine {
            path: Vec::new(),
            section_id: root.id,
            is_heading: false,
            body_idx: Some(i),
            line: l.clone(),
        });
    }
    if !root.body.is_empty() && !root.children.is_empty() {
        out.push(spacer(Vec::new(), root.id));
    }
    for (i, child) in root.children.iter().enumerate() {
        flatten_into(child, vec![i], &mut out);
        if i + 1 < root.children.len() {
            out.push(spacer(vec![i], child.id));
        }
    }
    out
}

fn flatten_into(s: &Section, path: Vec<usize>, out: &mut Vec<FlatLine>) {
    out.push(FlatLine {
        path: path.clone(),
        section_id: s.id,
        is_heading: true,
        body_idx: None,
        line: s.heading.clone(),
    });
    if s.collapsed {
        return;
    }
    for (i, l) in s.body.iter().enumerate() {
        out.push(FlatLine {
            path: path.clone(),
            section_id: s.id,
            is_heading: false,
            body_idx: Some(i),
            line: l.clone(),
        });
    }
    for (i, child) in s.children.iter().enumerate() {
        let mut p = path.clone();
        p.push(i);
        flatten_into(child, p, out);
    }
}

fn spacer(path: Vec<usize>, id: SectionId) -> FlatLine {
    FlatLine {
        path,
        section_id: id,
        is_heading: false,
        body_idx: None,
        line: Line::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Section {
        let mut root = Section::root();
        let mut a = Section::new(0, "a", SectionValue::Group(Group::Unstaged), "A".into());
        a.body.push("a1".into());
        let mut b = Section::new(a.id, "b", SectionValue::Text, "B".into());
        b.body.push("b1".into());
        b.body.push("b2".into());
        a.children.push(b);
        root.children.push(a);
        root.children
            .push(Section::new(0, "c", SectionValue::Text, "C".into()));
        root
    }

    #[test]
    fn flatten_visits_depth_first_with_spacers() {
        let flat = flatten(&tree());
        let texts: Vec<String> = flat.iter().map(|f| f.line.to_string()).collect();
        assert_eq!(texts, vec!["A", "a1", "B", "b1", "b2", "", "C"]);
        assert!(flat[0].is_heading);
        assert_eq!(flat[2].path, vec![0, 0]);
        assert_eq!(flat[4].body_idx, Some(1));
    }

    #[test]
    fn collapse_hides_descendants() {
        let mut t = tree();
        t.children[0].collapsed = true;
        let texts: Vec<String> = flatten(&t).iter().map(|f| f.line.to_string()).collect();
        assert_eq!(texts, vec!["A", "", "C"]);
    }

    #[test]
    fn collapse_state_survives_rebuild() {
        let mut old = tree();
        old.children[0].children[0].collapsed = true;
        let mut new = tree();
        new.inherit_collapse(&old);
        assert!(new.children[0].children[0].collapsed);
    }

    #[test]
    fn ids_are_stable_and_hierarchical() {
        let a = section_id(0, "file:src/a.rs");
        let b = section_id(0, "file:src/a.rs");
        assert_eq!(a, b);
        assert_ne!(section_id(a, "hunk:1"), section_id(b, "hunk:2"));
    }
}
