//! Pure domain types shared by the git layer and the UI.

use std::sync::Arc;

/// Which "diff area" a file or hunk belongs to. Commands dispatch on this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiffArea {
    Untracked,
    Unstaged,
    Staged,
    /// Read-only diffs (revision buffers). Not a target for stage/unstage.
    Committed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    /// The full `@@ -a,b +c,d @@ ctx` line as emitted by git.
    pub header: String,
    /// Content lines including their `' '`/`'+'`/`'-'`/`'\\'` prefix.
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileDiff {
    pub path: String,
    /// Set for renames: the pre-rename path.
    pub old_path: Option<String>,
    pub is_new: bool,
    pub is_deleted: bool,
    pub is_binary: bool,
    pub hunks: Vec<Hunk>,
}

impl FileDiff {
    pub fn status_word(&self) -> &'static str {
        if self.is_new {
            "new file"
        } else if self.is_deleted {
            "deleted "
        } else if self.old_path.is_some() {
            "renamed "
        } else {
            "modified"
        }
    }
}

/// One row of the log buffer and of the status "Recent commits" list (which
/// leaves `author`/`date` empty).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    /// Abbreviated hash.
    pub hash: String,
    /// Ref decorations (`%D`): branch/tag/HEAD names, empty when undecorated.
    pub refs: String,
    pub subject: String,
    pub author: String,
    /// Relative author date (`%ar`), e.g. "3 days ago".
    pub date: String,
}

/// What kind of ref a `RefEntry` names, deciding its group and color in the
/// references buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    LocalBranch,
    RemoteBranch,
    Tag,
}

/// One row of the references buffer: a branch, remote-tracking branch or tag
/// with the commit it points at and (for branches) its upstream tracking
/// state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefEntry {
    pub kind: RefKind,
    /// Short name, e.g. "main", "origin/main", "v1.0".
    pub name: String,
    /// Abbreviated object id.
    pub hash: String,
    /// Commit (or annotated-tag) subject.
    pub subject: String,
    /// `upstream:short` — the tracked branch, empty when none.
    pub upstream: String,
    /// `upstream:track` — e.g. "[ahead 1, behind 2]" or "[gone]", empty when none.
    pub track: String,
    /// The currently checked-out branch (`%(HEAD)` == "*").
    pub is_head: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StashInfo {
    pub index: usize,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BranchInfo {
    /// Branch name, `None` when detached.
    pub head: Option<String>,
    pub detached: bool,
    /// HEAD commit id, `None` on an unborn branch.
    pub oid: Option<String>,
    pub upstream: Option<String>,
    pub ahead: i64,
    pub behind: i64,
}

/// Everything the status buffer needs, read in one refresh pass.
#[derive(Debug, Clone, Default)]
pub struct StatusSnapshot {
    pub branch: BranchInfo,
    /// `<short-hash> <subject>` of HEAD, if any.
    pub head_summary: Option<String>,
    /// In-progress operation ("merging", "rebasing", ...), if any.
    pub state: Option<String>,
    pub untracked: Vec<String>,
    pub unmerged: Vec<String>,
    /// `Arc`-shared with the status pane, which needs the same diffs for
    /// dispatch — a large worktree diff must not be deep-copied per refresh.
    pub unstaged: Arc<Vec<FileDiff>>,
    pub staged: Arc<Vec<FileDiff>>,
    pub stashes: Vec<StashInfo>,
    pub recent: Vec<LogEntry>,
}
