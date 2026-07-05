//! The section tree — Magit's core abstraction. Every buffer is a tree of
//! sections; commands act on the section at point (DWIM dispatch).

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use ratatui::style::Color;
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
    /// Lines are `Rc`-shared with `FlatLine` so that flattening — which runs
    /// on every refresh and fold toggle — never copies line contents.
    pub heading: Rc<Line<'static>>,
    pub body: Vec<Rc<Line<'static>>>,
    pub children: Vec<Section>,
    pub collapsed: bool,
    /// When set on the root, its children render as one tight list: no
    /// blank-line separators between them, and no blank line between the root
    /// body (e.g. the log header) and the first child. Used by the log.
    pub compact: bool,
    /// When set, this section's body lines render as a full-width bar in this
    /// background color (the log header). The color still comes from `Theme`.
    pub body_fill: Option<Color>,
    /// Optional right-aligned trailer for this section's heading line (the log
    /// margin: author + date). Placed flush to the buffer's right edge at
    /// render time, since layout needs the viewport width.
    pub margin: Option<Line<'static>>,
}

impl Section {
    pub fn root() -> Self {
        Self {
            id: 0,
            value: SectionValue::Root,
            heading: Rc::default(),
            body: Vec::new(),
            children: Vec::new(),
            collapsed: false,
            compact: false,
            body_fill: None,
            margin: None,
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
            heading: Rc::new(heading),
            body: Vec::new(),
            children: Vec::new(),
            collapsed: false,
            compact: false,
            body_fill: None,
            margin: None,
        }
    }

    /// Append a body line (wraps it in the `Rc` the tree stores).
    pub fn push_body(&mut self, line: Line<'static>) {
        self.body.push(Rc::new(line));
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
        let mut states = HashMap::new();
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

fn collect_collapse(s: &Section, out: &mut HashMap<SectionId, bool>) {
    out.insert(s.id, s.collapsed);
    for c in &s.children {
        collect_collapse(c, out);
    }
}

fn apply_collapse(s: &mut Section, states: &HashMap<SectionId, bool>) {
    if let Some(&collapsed) = states.get(&s.id) {
        s.collapsed = collapsed;
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
    /// Shared with the owning section — flattening copies pointers, not text.
    pub line: Rc<Line<'static>>,
    /// Right-aligned trailer, rendered flush to the viewport's right edge
    /// (only ever set on a heading line — the log margin).
    pub margin: Option<Line<'static>>,
    /// When set, extend this line's background across the full pane width in
    /// this color (the log header bar).
    pub fill_bg: Option<Color>,
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
            margin: None,
            fill_bg: root.body_fill,
        });
    }
    if !root.body.is_empty() && !root.children.is_empty() && !root.compact {
        out.push(spacer(Vec::new(), root.id));
    }
    for (i, child) in root.children.iter().enumerate() {
        flatten_into(child, vec![i], &mut out);
        if i + 1 < root.children.len() && !root.compact {
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
        margin: s.margin.clone(),
        fill_bg: None,
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
            margin: None,
            fill_bg: None,
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
        line: Rc::default(),
        margin: None,
        fill_bg: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Section {
        let mut root = Section::root();
        let mut a = Section::new(0, "a", SectionValue::Group(Group::Unstaged), "A".into());
        a.push_body("a1".into());
        let mut b = Section::new(a.id, "b", SectionValue::Text, "B".into());
        b.push_body("b1".into());
        b.push_body("b2".into());
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
    fn compact_root_omits_inter_child_spacers() {
        let mut t = tree();
        t.compact = true;
        let texts: Vec<String> = flatten(&t).iter().map(|f| f.line.to_string()).collect();
        // Same as the default flatten but without the "" spacer before "C".
        assert_eq!(texts, vec!["A", "a1", "B", "b1", "b2", "C"]);
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
