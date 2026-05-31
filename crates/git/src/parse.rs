//! Pure parsers for git's machine-readable output. No process execution, so the
//! tests here are hermetic and run on CI.

/// One entry from `git status --porcelain` (v1 format: `XY <path>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    /// Two-character status code, e.g. `" M"`, `"??"`, `"A "`.
    pub code: String,
    /// Path the status applies to. For renames git emits `old -> new`; that is
    /// left verbatim here.
    pub path: String,
}

/// A commit, parsed from a `\x1f`-delimited `git log` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    /// Full commit hash (`%H`).
    pub hash: String,
    /// Abbreviated commit hash (`%h`).
    pub short_hash: String,
    /// Author name (`%an`).
    pub author: String,
    /// Author date, strict ISO-8601 (`%aI`), e.g. `2026-05-31T10:00:00+00:00`.
    pub date: String,
    /// Subject line (`%s`).
    pub subject: String,
}

/// A local branch from `git branch`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    /// Branch name.
    pub name: String,
    /// Whether this is the checked-out branch (the `*` marker).
    pub current: bool,
}

/// Parse `git status --porcelain` output.
pub(crate) fn parse_porcelain(output: &str) -> Vec<StatusEntry> {
    output
        .lines()
        // "XY path" — two code chars, a separator, then a non-empty path.
        .filter(|line| line.len() >= 4)
        .map(|line| StatusEntry {
            code: line[..2].to_string(),
            path: line[3..].to_string(),
        })
        .collect()
}

/// Parse `git log --format=%H%x1f%h%x1f%an%x1f%aI%x1f%s` output (one commit per
/// line, fields separated by the ASCII unit separator).
pub(crate) fn parse_log(output: &str) -> Vec<Commit> {
    output
        .lines()
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let mut fields = line.split('\u{1f}');
            Some(Commit {
                hash: fields.next()?.to_string(),
                short_hash: fields.next()?.to_string(),
                author: fields.next()?.to_string(),
                date: fields.next()?.to_string(),
                subject: fields.next().unwrap_or("").to_string(),
            })
        })
        .collect()
}

/// Parse `git branch` output. The first column is the `* `/`  `/`+ ` marker.
pub(crate) fn parse_branches(output: &str) -> Vec<Branch> {
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let current = line.starts_with('*');
            let name = line.get(1..).unwrap_or("").trim();
            // Skip the detached-HEAD pseudo-entry, e.g. "* (HEAD detached at …)".
            if name.is_empty() || name.starts_with('(') {
                return None;
            }
            Some(Branch {
                name: name.to_string(),
                current,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain_parses_codes_and_paths() {
        let got = parse_porcelain(" M src/lib.rs\n?? new.txt\nA  added.rs\n");
        assert_eq!(
            got,
            vec![
                StatusEntry {
                    code: " M".into(),
                    path: "src/lib.rs".into()
                },
                StatusEntry {
                    code: "??".into(),
                    path: "new.txt".into()
                },
                StatusEntry {
                    code: "A ".into(),
                    path: "added.rs".into()
                },
            ]
        );
    }

    #[test]
    fn porcelain_ignores_blank_and_short_lines() {
        assert!(parse_porcelain("\n  \nX\n").is_empty());
    }

    #[test]
    fn log_splits_unit_separated_fields() {
        let input = "abc123\u{1f}abc\u{1f}Ada\u{1f}2026-05-31T10:00:00+00:00\u{1f}Add feature\n\
                     def456\u{1f}def\u{1f}Linus\u{1f}2026-05-30T09:00:00+00:00\u{1f}Fix bug\n";
        let got = parse_log(input);
        assert_eq!(got.len(), 2);
        assert_eq!(
            got[0],
            Commit {
                hash: "abc123".into(),
                short_hash: "abc".into(),
                author: "Ada".into(),
                date: "2026-05-31T10:00:00+00:00".into(),
                subject: "Add feature".into(),
            }
        );
        assert_eq!(got[1].subject, "Fix bug");
    }

    #[test]
    fn log_tolerates_empty_subject() {
        let got = parse_log("h\u{1f}h\u{1f}A\u{1f}2026-05-31T10:00:00+00:00\u{1f}\n");
        assert_eq!(got[0].subject, "");
    }

    #[test]
    fn branches_marks_current_and_skips_detached() {
        let got = parse_branches("* main\n  feature\n  (HEAD detached at abc123)\n");
        assert_eq!(
            got,
            vec![
                Branch {
                    name: "main".into(),
                    current: true
                },
                Branch {
                    name: "feature".into(),
                    current: false
                },
            ]
        );
    }
}
