//! Building patches for `git apply --cached` — the machinery behind hunk- and
//! line-level staging. Pure functions, heavily tested: this is the code path
//! where a bug corrupts the index.

use super::types::{FileDiff, Hunk};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineOp {
    /// Patch will be applied forward to the index (`git apply --cached`).
    Stage,
    /// Patch will be applied in reverse to the index (`git apply -R --cached`).
    Unstage,
}

/// `---`/`+++` header for a patch touching `fd`.
pub fn file_header(fd: &FileDiff) -> String {
    let a = if fd.is_new {
        "/dev/null".to_string()
    } else {
        format!("a/{}", fd.old_path.as_deref().unwrap_or(&fd.path))
    };
    let b = if fd.is_deleted {
        "/dev/null".to_string()
    } else {
        format!("b/{}", fd.path)
    };
    format!("--- {a}\n+++ {b}\n")
}

/// A patch containing the whole hunk verbatim.
pub fn hunk_patch(fd: &FileDiff, hunk: &Hunk) -> String {
    let mut out = file_header(fd);
    out.push_str(&hunk.header);
    out.push('\n');
    for l in &hunk.lines {
        out.push_str(l);
        out.push('\n');
    }
    out
}

/// A patch containing only the single `+`/`-` line at `keep` (an index into
/// `hunk.lines`); all other lines are dropped or turned into context so the
/// patch stays self-consistent. Returns `None` when the target is a context
/// line — callers should fall back to staging the whole hunk.
///
/// The transform depends on the apply direction:
/// - `Stage` (forward onto the index): unselected `+` lines don't exist in the
///   index yet → drop; unselected `-` lines still exist there → context.
/// - `Unstage` (reverse onto the index): unselected `+` lines exist in the
///   index → context; unselected `-` lines don't → drop.
pub fn line_patch(fd: &FileDiff, hunk: &Hunk, keep: usize, op: LineOp) -> Option<String> {
    let target = hunk.lines.get(keep)?;
    if !matches!(target.as_bytes().first(), Some(b'+' | b'-')) {
        return None;
    }

    let mut body = String::new();
    let (mut old_count, mut new_count) = (0u32, 0u32);
    let mut last_kept = false;
    for (i, line) in hunk.lines.iter().enumerate() {
        let mut push = |l: &str| {
            body.push_str(l);
            body.push('\n');
        };
        match line.as_bytes().first() {
            // "\ No newline at end of file" annotates the preceding line.
            Some(b'\\') => {
                if last_kept {
                    push(line);
                }
                continue;
            }
            Some(b' ') => {
                push(line);
                old_count += 1;
                new_count += 1;
                last_kept = true;
            }
            Some(b'+') => {
                if i == keep {
                    push(line);
                    new_count += 1;
                    last_kept = true;
                } else if op == LineOp::Unstage {
                    push(&format!(" {}", &line[1..]));
                    old_count += 1;
                    new_count += 1;
                    last_kept = true;
                } else {
                    last_kept = false;
                }
            }
            Some(b'-') => {
                if i == keep {
                    push(line);
                    old_count += 1;
                    last_kept = true;
                } else if op == LineOp::Stage {
                    push(&format!(" {}", &line[1..]));
                    old_count += 1;
                    new_count += 1;
                    last_kept = true;
                } else {
                    last_kept = false;
                }
            }
            _ => {}
        }
    }

    let mut out = file_header(fd);
    out.push_str(&format!(
        "@@ -{},{} +{},{} @@\n",
        hunk.old_start, old_count, hunk.new_start, new_count
    ));
    out.push_str(&body);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hunk(lines: &[&str]) -> Hunk {
        Hunk {
            old_start: 10,
            old_count: 0, // counts unused by the builder
            new_start: 12,
            new_count: 0,
            header: "@@ -10,3 +12,4 @@".into(),
            lines: lines.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn fd(path: &str) -> FileDiff {
        FileDiff {
            path: path.into(),
            ..Default::default()
        }
    }

    #[test]
    fn full_hunk_patch() {
        let p = hunk_patch(&fd("a.rs"), &hunk(&[" ctx", "+add", "-del"]));
        assert_eq!(
            p,
            "--- a/a.rs\n+++ b/a.rs\n@@ -10,3 +12,4 @@\n ctx\n+add\n-del\n"
        );
    }

    #[test]
    fn stage_single_added_line() {
        // Staging only "+two": "+one" is dropped, "-gone" becomes context.
        let h = hunk(&[" a", "+one", "+two", "-gone", " b"]);
        let p = line_patch(&fd("f"), &h, 2, LineOp::Stage).unwrap();
        assert_eq!(
            p,
            "--- a/f\n+++ b/f\n@@ -10,3 +12,4 @@\n a\n+two\n gone\n b\n"
        );
    }

    #[test]
    fn unstage_single_added_line() {
        // Reverse-applying to the index: other "+" lines stay (context),
        // "-" lines are absent from the index (dropped).
        let h = hunk(&[" a", "+one", "+two", "-gone", " b"]);
        let p = line_patch(&fd("f"), &h, 1, LineOp::Unstage).unwrap();
        assert_eq!(
            p,
            "--- a/f\n+++ b/f\n@@ -10,3 +12,4 @@\n a\n+one\n two\n b\n"
        );
    }

    #[test]
    fn stage_single_removed_line() {
        let h = hunk(&["-x", "-y", " z"]);
        let p = line_patch(&fd("f"), &h, 1, LineOp::Stage).unwrap();
        assert_eq!(p, "--- a/f\n+++ b/f\n@@ -10,3 +12,2 @@\n x\n-y\n z\n");
    }

    #[test]
    fn context_line_returns_none() {
        let h = hunk(&[" a", "+b"]);
        assert!(line_patch(&fd("f"), &h, 0, LineOp::Stage).is_none());
    }

    #[test]
    fn no_newline_marker_follows_kept_line() {
        let h = hunk(&["-old", "+new", "\\ No newline at end of file"]);
        let p = line_patch(&fd("f"), &h, 0, LineOp::Stage).unwrap();
        // "+new" was dropped, so the trailing "\" marker must go too.
        assert_eq!(p, "--- a/f\n+++ b/f\n@@ -10,1 +12,0 @@\n-old\n");
    }

    #[test]
    fn new_file_header_uses_dev_null() {
        let mut f = fd("n.txt");
        f.is_new = true;
        assert_eq!(file_header(&f), "--- /dev/null\n+++ b/n.txt\n");
    }
}
