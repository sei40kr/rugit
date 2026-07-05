//! Pure parsers for git plumbing/porcelain output. No I/O here — everything is
//! unit-testable against fixture strings.

use super::types::{BranchInfo, FileDiff, Hunk, LogEntry, StashInfo};

/// Result of parsing `git status --porcelain=v2 --branch -z`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct StatusV2 {
    pub branch: BranchInfo,
    pub untracked: Vec<String>,
    pub unmerged: Vec<String>,
}

pub fn parse_status_v2(output: &str) -> StatusV2 {
    let mut st = StatusV2::default();
    let mut tokens = output.split('\0').filter(|t| !t.is_empty());
    while let Some(tok) = tokens.next() {
        if let Some(rest) = tok.strip_prefix("# ") {
            parse_branch_header(rest, &mut st.branch);
        } else if let Some(rest) = tok.strip_prefix("? ") {
            st.untracked.push(rest.to_string());
        } else if tok.starts_with("1 ") {
            // Ordinary changed entry: 8 fixed fields, then the path.
            // We build diffs from `git diff`, so nothing to record here.
        } else if tok.starts_with("2 ") {
            // Rename/copy entry: the original path follows as its own
            // NUL-separated token — consume it.
            tokens.next();
        } else if let Some(rest) = tok.strip_prefix("u ") {
            if let Some(path) = nth_field_rest(rest, 9) {
                st.unmerged.push(path.to_string());
            }
        }
        // "! " (ignored entries) are skipped.
    }
    st
}

fn parse_branch_header(rest: &str, branch: &mut BranchInfo) {
    if let Some(v) = rest.strip_prefix("branch.oid ") {
        if v != "(initial)" {
            branch.oid = Some(v.to_string());
        }
    } else if let Some(v) = rest.strip_prefix("branch.head ") {
        if v == "(detached)" {
            branch.detached = true;
        } else {
            branch.head = Some(v.to_string());
        }
    } else if let Some(v) = rest.strip_prefix("branch.upstream ") {
        branch.upstream = Some(v.to_string());
    } else if let Some(v) = rest.strip_prefix("branch.ab ") {
        for part in v.split_whitespace() {
            if let Some(n) = part.strip_prefix('+') {
                branch.ahead = n.parse().unwrap_or(0);
            } else if let Some(n) = part.strip_prefix('-') {
                branch.behind = n.parse().unwrap_or(0);
            }
        }
    }
}

/// Skip `n` space-separated fields and return the remainder of the line.
fn nth_field_rest(s: &str, n: usize) -> Option<&str> {
    let mut rest = s;
    for _ in 0..n {
        rest = rest.split_once(' ')?.1;
    }
    Some(rest)
}

/// Parse `git diff` unified output into per-file diffs with hunks.
pub fn parse_diff(text: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut cur: Option<FileDiff> = None;
    let mut cur_hunk: Option<Hunk> = None;

    let flush_hunk = |cur: &mut Option<FileDiff>, hunk: &mut Option<Hunk>| {
        if let (Some(f), Some(h)) = (cur.as_mut(), hunk.take()) {
            f.hunks.push(h);
        }
    };

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            flush_hunk(&mut cur, &mut cur_hunk);
            if let Some(f) = cur.take() {
                files.push(f);
            }
            let mut fd = FileDiff::default();
            // Fallback path from the `diff --git a/X b/Y` line; the
            // `+++`/`---` header lines below take precedence.
            if let Some((_, b)) = rest.split_once(" b/") {
                fd.path = unquote(b);
            }
            cur = Some(fd);
            continue;
        }
        let Some(fd) = cur.as_mut() else { continue };

        if let Some(hunk) = cur_hunk
            .as_mut()
            .filter(|_| matches!(line.as_bytes().first(), Some(b' ' | b'+' | b'-' | b'\\')))
        {
            hunk.lines.push(line.to_string());
        } else if line.starts_with("@@") {
            flush_hunk(&mut cur, &mut cur_hunk);
            cur_hunk = parse_hunk_header(line);
        } else if line.starts_with("new file mode") {
            fd.is_new = true;
        } else if line.starts_with("deleted file mode") {
            fd.is_deleted = true;
        } else if let Some(p) = line.strip_prefix("rename from ") {
            fd.old_path = Some(unquote(p));
        } else if let Some(p) = line.strip_prefix("rename to ") {
            fd.path = unquote(p);
        } else if line.starts_with("Binary files ") || line.starts_with("GIT binary patch") {
            fd.is_binary = true;
        } else if let Some(p) = line.strip_prefix("--- a/") {
            if fd.old_path.is_none() && !fd.is_new {
                fd.old_path = Some(unquote(p)).filter(|op| *op != fd.path);
            }
        } else if let Some(p) = line.strip_prefix("+++ b/") {
            fd.path = unquote(p);
        }
        // "index ...", "old mode", "similarity index", "--- /dev/null",
        // "+++ /dev/null" need no handling.
    }
    flush_hunk(&mut cur, &mut cur_hunk);
    if let Some(f) = cur.take() {
        files.push(f);
    }
    files
}

/// Parse `@@ -a,b +c,d @@ ctx` (counts default to 1 when omitted).
fn parse_hunk_header(line: &str) -> Option<Hunk> {
    let rest = line.strip_prefix("@@ -")?;
    let (old, rest) = rest.split_once(" +")?;
    let (new, _) = rest.split_once(" @@")?;
    let parse_pair = |s: &str| -> Option<(u32, u32)> {
        match s.split_once(',') {
            Some((a, b)) => Some((a.parse().ok()?, b.parse().ok()?)),
            None => Some((s.parse().ok()?, 1)),
        }
    };
    let (old_start, old_count) = parse_pair(old)?;
    let (new_start, new_count) = parse_pair(new)?;
    Some(Hunk {
        old_start,
        old_count,
        new_start,
        new_count,
        header: line.to_string(),
        lines: Vec::new(),
    })
}

/// Minimal unquoting of git-quoted paths (`"path with \"quotes\""`).
fn unquote(s: &str) -> String {
    if !(s.starts_with('"') && s.ends_with('"') && s.len() >= 2) {
        return s.to_string();
    }
    let mut out = String::new();
    let mut chars = s[1..s.len() - 1].chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Parse `git log --format=%h%x1f%D%x1f%s%x1f%an%x1f%ar` (one commit per
/// line, fields separated by US, 0x1f). Missing trailing fields are empty, so
/// this also accepts the shorter `%h%x1f%D%x1f%s` used for recent commits.
pub fn parse_log_entries(text: &str) -> Vec<LogEntry> {
    text.lines()
        .filter_map(|l| {
            let mut f = l.split('\x1f');
            let hash = f.next()?.to_string();
            if hash.is_empty() {
                return None;
            }
            Some(LogEntry {
                hash,
                refs: f.next().unwrap_or("").to_string(),
                subject: f.next().unwrap_or("").to_string(),
                author: f.next().unwrap_or("").to_string(),
                date: f.next().unwrap_or("").to_string(),
            })
        })
        .collect()
}

/// Parse `git stash list --format=%gd%x1f%s`.
pub fn parse_stash_list(text: &str) -> Vec<StashInfo> {
    text.lines()
        .filter_map(|l| {
            let (gd, msg) = l.split_once('\x1f')?;
            let index = gd
                .strip_prefix("stash@{")?
                .strip_suffix('}')?
                .parse()
                .ok()?;
            Some(StashInfo {
                index,
                message: msg.to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_v2_branch_and_untracked() {
        let raw = "# branch.oid 1234abcd\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -1\x001 .M N... 100644 100644 100644 aaa bbb src/main.rs\0? newfile.txt\0";
        let st = parse_status_v2(raw);
        assert_eq!(st.branch.head.as_deref(), Some("main"));
        assert_eq!(st.branch.oid.as_deref(), Some("1234abcd"));
        assert_eq!(st.branch.upstream.as_deref(), Some("origin/main"));
        assert_eq!((st.branch.ahead, st.branch.behind), (2, 1));
        assert_eq!(st.untracked, vec!["newfile.txt"]);
    }

    #[test]
    fn status_v2_initial_and_rename() {
        let raw = "# branch.oid (initial)\0# branch.head main\x002 R. N... 100644 100644 100644 aaa bbb R100 new.rs\0old.rs\0? x\0";
        let st = parse_status_v2(raw);
        assert_eq!(st.branch.oid, None);
        // The rename's second token must not be mistaken for an entry.
        assert_eq!(st.untracked, vec!["x"]);
    }

    #[test]
    fn diff_two_files_with_hunks() {
        let raw = "\
diff --git a/src/a.rs b/src/a.rs
index 111..222 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,3 +1,4 @@ fn main
 line1
+added
 line2
 line3
@@ -10,2 +11,2 @@
-old
+new
 ctx
diff --git a/b.txt b/b.txt
new file mode 100644
--- /dev/null
+++ b/b.txt
@@ -0,0 +1 @@
+hello
";
        let files = parse_diff(raw);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "src/a.rs");
        assert_eq!(files[0].hunks.len(), 2);
        assert_eq!(files[0].hunks[0].old_start, 1);
        assert_eq!(files[0].hunks[0].new_count, 4);
        assert_eq!(files[0].hunks[0].lines.len(), 4);
        assert_eq!(files[0].hunks[1].lines, vec!["-old", "+new", " ctx"]);
        assert!(files[1].is_new);
        assert_eq!(files[1].path, "b.txt");
        assert_eq!(files[1].hunks[0].lines, vec!["+hello"]);
    }

    #[test]
    fn diff_rename_and_binary() {
        let raw = "\
diff --git a/old.rs b/new.rs
similarity index 90%
rename from old.rs
rename to new.rs
diff --git a/img.png b/img.png
index 111..222 100644
Binary files a/img.png and b/img.png differ
";
        let files = parse_diff(raw);
        assert_eq!(files[0].old_path.as_deref(), Some("old.rs"));
        assert_eq!(files[0].path, "new.rs");
        assert!(files[1].is_binary);
    }

    #[test]
    fn hunk_header_without_counts() {
        let h = parse_hunk_header("@@ -5 +7 @@").unwrap();
        assert_eq!(
            (h.old_start, h.old_count, h.new_start, h.new_count),
            (5, 1, 7, 1)
        );
    }

    #[test]
    fn stash_list() {
        assert_eq!(
            parse_stash_list("stash@{0}\x1fWIP on main\n"),
            vec![StashInfo {
                index: 0,
                message: "WIP on main".into()
            }]
        );
    }

    #[test]
    fn log_entries_with_and_without_decorations() {
        let raw = "abc123\x1fHEAD -> main, origin/main\x1ffix: thing\x1fAda\x1f2 days ago\n\
                   def456\x1f\x1fplain commit\x1fGrace\x1f3 weeks ago\n";
        let entries = parse_log_entries(raw);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].hash, "abc123");
        assert_eq!(entries[0].refs, "HEAD -> main, origin/main");
        assert_eq!(entries[0].subject, "fix: thing");
        assert_eq!(entries[0].author, "Ada");
        assert_eq!(entries[0].date, "2 days ago");
        assert_eq!(entries[1].refs, "");
        assert_eq!(entries[1].subject, "plain commit");
    }
}
