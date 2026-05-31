//! Pure parsers for jj output. No process execution, so these tests are
//! hermetic and run on CI.

/// A jj change, parsed from a `\t`-delimited template row.
#[derive(Debug, Clone, PartialEq, Eq)]
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
pub struct Bookmark {
    /// Bookmark name.
    pub name: String,
    /// Short id of the commit it points at.
    pub target: String,
}

/// Template used by the change commands: tab-separated, one change per line.
pub(crate) const CHANGE_TEMPLATE: &str = "change_id.short() ++ \"\\t\" ++ commit_id.short() ++ \"\\t\" ++ if(empty, \"true\", \"false\") ++ \"\\t\" ++ description.first_line() ++ \"\\n\"";

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
}
