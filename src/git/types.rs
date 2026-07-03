//! Pure domain types shared by the git layer and the UI.

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitInfo {
    pub hash: String,
    pub subject: String,
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
    pub unstaged: Vec<FileDiff>,
    pub staged: Vec<FileDiff>,
    pub stashes: Vec<StashInfo>,
    pub recent: Vec<CommitInfo>,
}
