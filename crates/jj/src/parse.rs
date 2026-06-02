//! Pure parsers for jj output. No process execution, so these tests are
//! hermetic and run on CI.

/// A jj change, parsed from a `\t`-delimited template row.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Change {
    /// Short change id (`change_id.short()`).
    pub change_id: String,
    /// Short commit id (`commit_id.short()`).
    pub commit_id: String,
    /// `true` when the change makes no file modifications.
    pub empty: bool,
    /// First line of the description (empty for an undescribed change).
    pub description: String,
}

/// A jj bookmark, parsed from `jj bookmark list` output.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Bookmark {
    /// Bookmark name.
    pub name: String,
    /// Short id of the commit it points at.
    pub target: String,
}

/// A workspace from `jj workspace list` (rendered with [`WORKSPACE_TEMPLATE`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Workspace {
    /// Workspace name (`default` for the main one).
    pub name: String,
    /// Short commit id of the workspace's working-copy commit.
    pub commit: String,
    /// Local bookmarks pointing at that commit (empty when none).
    pub bookmarks: Vec<String>,
}

/// One entry from `jj diff --summary`: a single-letter status (`M`/`A`/`D`/…)
/// and the path it applies to.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ChangedPath {
    /// Status letter (`M` modified, `A` added, `D` deleted, …).
    pub status: char,
    /// The path the status applies to.
    pub path: String,
}

/// Aggregate line/file counts from the `jj diff --stat` summary footer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct DiffStat {
    /// Number of files changed.
    pub files_changed: usize,
    /// Lines added (`insertions(+)`).
    pub insertions: usize,
    /// Lines removed (`deletions(-)`).
    pub deletions: usize,
}

/// How a file changed in a unified diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChangeKind {
    /// A new file (`new file mode …`).
    Added,
    /// An existing file's contents changed.
    Modified,
    /// The file was removed (`deleted file mode …`).
    Deleted,
    /// The file was renamed (`rename from …` / `rename to …`).
    Renamed,
}

/// One line inside a [`Hunk`], tagged by its role. The stored text excludes the
/// leading ` `/`+`/`-` marker.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DiffLine {
    /// Unchanged context line (leading ` `).
    Context(String),
    /// Added line (leading `+`).
    Added(String),
    /// Removed line (leading `-`).
    Removed(String),
}

/// A single `@@ … @@` hunk within a [`FileDiff`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Hunk {
    /// Start line in the old file (the `-<start>` of the `@@` header).
    pub old_start: usize,
    /// Line count in the old file (defaults to 1 when the `,<count>` is omitted).
    pub old_lines: usize,
    /// Start line in the new file (the `+<start>` of the `@@` header).
    pub new_start: usize,
    /// Line count in the new file (defaults to 1 when the `,<count>` is omitted).
    pub new_lines: usize,
    /// Text after the closing `@@` (the function/section heading); empty when none.
    pub section: String,
    /// The hunk body, one entry per `+`/`-`/` ` line.
    pub lines: Vec<DiffLine>,
}

/// One file's entry in a parsed git-format unified diff (`jj diff --git`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FileDiff {
    /// How the file changed.
    pub change: ChangeKind,
    /// The file's path — the *new* path for a rename — forward-slash normalised.
    pub path: String,
    /// For a rename, the original path (forward-slash normalised); `None` otherwise.
    pub old_path: Option<String>,
    /// The `@@` hunks; empty for a binary file or a pure rename with no edits.
    pub hunks: Vec<Hunk>,
}

/// Template used by the change commands: tab-separated, one change per line.
pub(crate) const CHANGE_TEMPLATE: &str = "change_id.short() ++ \"\\t\" ++ commit_id.short() ++ \"\\t\" ++ if(empty, \"true\", \"false\") ++ \"\\t\" ++ description.first_line() ++ \"\\n\"";

/// `jj workspace list -T` template: `name\t<commit>\t<bookmarks,comma-joined>`.
pub(crate) const WORKSPACE_TEMPLATE: &str = "name ++ \"\\t\" ++ target.commit_id().short() ++ \"\\t\" ++ target.local_bookmarks().map(|b| b.name()).join(\",\") ++ \"\\n\"";

/// `jj log -T` template rendering a commit's local bookmark names, comma-joined.
/// Drives `current_bookmark`/`trunk`.
pub(crate) const BOOKMARKS_TEMPLATE: &str = "local_bookmarks.map(|b| b.name()).join(\",\")";

/// `jj log -T` template: `"1"` when the commit has a conflict, else `"0"`.
pub(crate) const CONFLICT_TEMPLATE: &str = "if(conflict, \"1\", \"0\")";

/// `jj log -T` template emitting one short commit id per line — for counting a
/// revset.
pub(crate) const COUNT_TEMPLATE: &str = "commit_id.short() ++ \"\\n\"";

/// Parse rows produced by [`CHANGE_TEMPLATE`].
pub(crate) fn parse_changes(output: &str) -> Vec<Change> {
    output
        .lines()
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let change_id = fields.next()?.to_string();
            let commit_id = fields.next()?.to_string();
            let empty = fields.next()? == "true";
            let description = fields.next().unwrap_or("").to_string();
            Some(Change {
                change_id,
                commit_id,
                empty,
                description,
            })
        })
        .collect()
}

/// Parse `jj bookmark list` default output. Local bookmark lines look like
/// `name: <change_id> <commit_id> <description>`; remote-tracking lines are
/// indented and skipped.
pub(crate) fn parse_bookmarks(output: &str) -> Vec<Bookmark> {
    output
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with(char::is_whitespace))
        .filter_map(|line| {
            let (name, rest) = line.split_once(':')?;
            // Tokens after the name are `<change_id> <commit_id> …`; take the
            // commit id (2nd), falling back to whatever is present.
            let mut tokens = rest.split_whitespace();
            let target = tokens
                .nth(1)
                .or_else(|| rest.split_whitespace().next())
                .unwrap_or("")
                .to_string();
            Some(Bookmark {
                name: name.trim().to_string(),
                target,
            })
        })
        .collect()
}

/// Parse rows produced by [`WORKSPACE_TEMPLATE`]: `name\t<commit>\t<bookmarks>`,
/// where bookmarks are comma-joined (and may be empty).
pub(crate) fn parse_workspaces(output: &str) -> Vec<Workspace> {
    output
        .lines()
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let mut fields = line.split('\t');
            let name = fields.next()?.to_string();
            let commit = fields.next().unwrap_or("").to_string();
            let bookmarks = fields
                .next()
                .unwrap_or("")
                .split(',')
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect();
            Some(Workspace {
                name,
                commit,
                bookmarks,
            })
        })
        .collect()
}

/// Parse `jj diff --summary`: each line is `<status-letter> <path>`.
pub(crate) fn parse_diff_summary(output: &str) -> Vec<ChangedPath> {
    output
        .lines()
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let mut chars = line.chars();
            let status = chars.next()?;
            // Skip the single separating space; the remainder is the raw path.
            let path = chars.as_str().strip_prefix(' ').unwrap_or(chars.as_str());
            if path.is_empty() {
                return None;
            }
            Some(ChangedPath {
                status,
                path: path.to_string(),
            })
        })
        .collect()
}

/// Parse the summary footer of `jj diff --stat`, e.g. `4 files changed, 157
/// insertions(+), 137 deletions(-)` (same shape as git's `--shortstat`). The
/// footer is the last line mentioning "changed"; no such line → all zeros.
pub(crate) fn parse_diff_stat(output: &str) -> DiffStat {
    let summary = output
        .lines()
        .rev()
        .find(|line| line.contains("changed"))
        .unwrap_or("");
    let mut stat = DiffStat::default();
    for part in summary.split(',') {
        let part = part.trim();
        let n = part
            .split_whitespace()
            .next()
            .and_then(|tok| tok.parse().ok())
            .unwrap_or(0);
        if part.contains("file") {
            stat.files_changed = n;
        } else if part.contains("insertion") {
            stat.insertions = n;
        } else if part.contains("deletion") {
            stat.deletions = n;
        }
    }
    stat
}

/// Parse a git-format unified diff (`jj diff --git`) into one [`FileDiff`] per
/// file. Public so a consumer can parse diff text it obtained by other means.
///
/// Paths are read from the unambiguous single-path lines (`+++ b/…`, `--- a/…`,
/// `rename to …`) rather than the space-ambiguous `diff --git a/… b/…` header,
/// and normalised to forward slashes. Ported from the `vcs-flow-commit` parser.
pub fn parse_diff(diff: &str) -> Vec<FileDiff> {
    diff_sections(diff).filter_map(parse_section).collect()
}

/// Slice a git-format diff into per-file sections (each starts at `diff --git`).
fn diff_sections(full: &str) -> impl Iterator<Item = &str> {
    let mut bounds = Vec::new();
    let mut idx = 0;
    for line in full.split_inclusive('\n') {
        if line.starts_with("diff --git ") {
            bounds.push(idx);
        }
        idx += line.len();
    }
    let ends = bounds
        .iter()
        .skip(1)
        .copied()
        .chain(std::iter::once(full.len()));
    bounds
        .clone()
        .into_iter()
        .zip(ends)
        .map(move |(s, e)| &full[s..e])
        .collect::<Vec<_>>()
        .into_iter()
}

/// Determine the [`FileDiff`] for one `diff --git` section: change kind and path
/// from the header lines, plus every `@@` hunk and its body.
fn parse_section(section: &str) -> Option<FileDiff> {
    let mut kind = ChangeKind::Modified;
    let mut new_path = None;
    let mut minus_path = None;
    let mut rename_to = None;
    let mut rename_from = None;
    let mut hunks: Vec<Hunk> = Vec::new();
    let mut current: Option<Hunk> = None;

    for line in section.lines() {
        if let Some(hunk) = parse_hunk_header(line) {
            if let Some(done) = current.replace(hunk) {
                hunks.push(done);
            }
            continue;
        }
        if let Some(hunk) = current.as_mut() {
            // Inside a hunk body: classify by the leading marker. `\ No newline at
            // end of file` annotations and any stray blank line are dropped.
            match line.as_bytes().first() {
                Some(b' ') => hunk.lines.push(DiffLine::Context(line[1..].to_string())),
                Some(b'+') => hunk.lines.push(DiffLine::Added(line[1..].to_string())),
                Some(b'-') => hunk.lines.push(DiffLine::Removed(line[1..].to_string())),
                _ => {}
            }
            continue;
        }
        // Header region (before the first `@@`).
        if line.starts_with("new file") {
            kind = ChangeKind::Added;
        } else if line.starts_with("deleted file") {
            kind = ChangeKind::Deleted;
        } else if let Some(p) = line.strip_prefix("rename to ") {
            rename_to = Some(p.trim_end().to_string());
        } else if let Some(p) = line.strip_prefix("rename from ") {
            rename_from = Some(p.trim_end().to_string());
        } else if let Some(p) = line.strip_prefix("+++ b/") {
            new_path = Some(p.trim_end().to_string());
        } else if let Some(p) = line.strip_prefix("--- a/") {
            minus_path = Some(p.trim_end().to_string());
        }
    }
    if let Some(done) = current.take() {
        hunks.push(done);
    }

    let normalize = |p: String| p.replace('\\', "/");
    // A rename keeps its old path so a caller can record the deletion too.
    let old_path = if rename_to.is_some() {
        kind = ChangeKind::Renamed;
        rename_from.map(normalize)
    } else {
        None
    };
    let path = rename_to
        .or(new_path)
        .or(minus_path)
        .or_else(|| header_b_path(section))?;
    Some(FileDiff {
        change: kind,
        path: normalize(path),
        old_path,
        hunks,
    })
}

/// Parse a hunk header `@@ -<os>[,<ol>] +<ns>[,<nl>] @@[ <section>]` into an empty
/// [`Hunk`]; `None` for any other line.
fn parse_hunk_header(line: &str) -> Option<Hunk> {
    let rest = line.strip_prefix("@@ ")?;
    let (ranges, section) = rest.split_once(" @@")?;
    let mut parts = ranges.split_whitespace();
    let (old_start, old_lines) = parse_hunk_range(parts.next()?.strip_prefix('-')?);
    let (new_start, new_lines) = parse_hunk_range(parts.next()?.strip_prefix('+')?);
    Some(Hunk {
        old_start,
        old_lines,
        new_start,
        new_lines,
        section: section.strip_prefix(' ').unwrap_or(section).to_string(),
        lines: Vec::new(),
    })
}

/// Parse a `<start>[,<count>]` hunk range; an omitted count means 1 line.
fn parse_hunk_range(range: &str) -> (usize, usize) {
    match range.split_once(',') {
        Some((start, count)) => (start.parse().unwrap_or(0), count.parse().unwrap_or(0)),
        None => (range.parse().unwrap_or(0), 1),
    }
}

/// Fallback path extraction for sections with no `+++`/`---`/`rename` lines
/// (e.g. binary files): the `b/<new>` of the `diff --git` header. Ambiguous only
/// when a path contains the literal `" b/"`, which binary-with-spaces makes rare.
fn header_b_path(section: &str) -> Option<String> {
    let first = section.lines().next()?;
    let s = first.strip_prefix("diff --git ")?;
    let idx = s.find(" b/")?;
    Some(s[idx + 1..].strip_prefix("b/").unwrap_or("").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_split_tab_fields() {
        let input = "kztuxlro\t38e00654\tfalse\tfeat: stuff\nqpvuntsm\t6ecf997f\ttrue\t\n";
        let got = parse_changes(input);
        assert_eq!(got.len(), 2);
        assert_eq!(
            got[0],
            Change {
                change_id: "kztuxlro".into(),
                commit_id: "38e00654".into(),
                empty: false,
                description: "feat: stuff".into(),
            }
        );
        // Undescribed, empty change.
        assert!(got[1].empty);
        assert_eq!(got[1].description, "");
    }

    #[test]
    fn bookmarks_parse_name_and_commit_and_skip_remotes() {
        let input = "main: pzlznprr f5d07685 feat(process): job-backed spawn\n  @origin: pzlznprr f5d07685 feat(process)\nfeature: abcd1234 deadbeef wip\n";
        let got = parse_bookmarks(input);
        assert_eq!(
            got,
            vec![
                Bookmark {
                    name: "main".into(),
                    target: "f5d07685".into()
                },
                Bookmark {
                    name: "feature".into(),
                    target: "deadbeef".into()
                },
            ]
        );
    }

    #[test]
    fn workspaces_split_tab_fields_and_bookmarks() {
        let input = "default\te2aa3420\tmain,feature\nws1\t12345678\t\n";
        let got = parse_workspaces(input);
        assert_eq!(got.len(), 2);
        assert_eq!(
            got[0],
            Workspace {
                name: "default".into(),
                commit: "e2aa3420".into(),
                bookmarks: vec!["main".into(), "feature".into()],
            }
        );
        // No bookmarks → empty vec, not [""].
        assert!(got[1].bookmarks.is_empty());
    }

    #[test]
    fn diff_summary_splits_status_and_path() {
        let got = parse_diff_summary("M src/lib.rs\nA new file.txt\nD gone.rs\n");
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].status, 'M');
        assert_eq!(got[1].path, "new file.txt");
        assert_eq!(got[2].status, 'D');
    }

    #[test]
    fn diff_stat_parses_footer_among_per_file_lines() {
        let input = "README.md | 10 +++---\n\
                     src/lib.rs | 4 +-\n\
                     4 files changed, 157 insertions(+), 137 deletions(-)\n";
        assert_eq!(
            parse_diff_stat(input),
            DiffStat {
                files_changed: 4,
                insertions: 157,
                deletions: 137
            }
        );
        assert_eq!(parse_diff_stat(""), DiffStat::default());
    }

    #[test]
    fn diff_covers_add_modify_delete_rename() {
        // jj `diff --git` is git-format, so the same fixture applies.
        let full = concat!(
            "diff --git a/new b/new\n",
            "new file mode 100644\n--- /dev/null\n+++ b/new\n@@ -0,0 +1 @@\n+n\n",
            "diff --git a/mod b/mod\n",
            "--- a/mod\n+++ b/mod\n@@ -1 +1 @@\n-a\n+b\n",
            "diff --git a/gone b/gone\n",
            "deleted file mode 100644\n--- a/gone\n+++ /dev/null\n@@ -1 +0,0 @@\n-x\n",
            "diff --git a/old/f.txt b/new/f.txt\n",
            "similarity index 100%\nrename from old/f.txt\nrename to new/f.txt\n",
        );
        let files = parse_diff(full);
        let kinds: Vec<_> = files.iter().map(|f| (f.path.as_str(), f.change)).collect();
        assert_eq!(
            kinds,
            vec![
                ("new", ChangeKind::Added),
                ("mod", ChangeKind::Modified),
                ("gone", ChangeKind::Deleted),
                ("new/f.txt", ChangeKind::Renamed),
            ]
        );
        let rename = files
            .iter()
            .find(|f| f.change == ChangeKind::Renamed)
            .unwrap();
        assert_eq!(rename.old_path.as_deref(), Some("old/f.txt"));
    }

    #[test]
    fn diff_parses_hunk_ranges_and_body() {
        let full = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,2 +1,3 @@ fn main()\n ctx\n-old\n+new\n+added\n";
        let files = parse_diff(full);
        assert_eq!(files.len(), 1);
        let hunk = &files[0].hunks[0];
        assert_eq!(
            (
                hunk.old_start,
                hunk.old_lines,
                hunk.new_start,
                hunk.new_lines
            ),
            (1, 2, 1, 3)
        );
        assert_eq!(hunk.section, "fn main()");
        assert_eq!(
            hunk.lines,
            vec![
                DiffLine::Context("ctx".into()),
                DiffLine::Removed("old".into()),
                DiffLine::Added("new".into()),
                DiffLine::Added("added".into()),
            ]
        );
    }
}
