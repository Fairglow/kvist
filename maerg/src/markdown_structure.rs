//! Native Markdown structure, with fenced examples excluded from headings.

/// One physical line and its structural role.
pub(crate) struct Line<'a> {
    pub text: &'a str,
    pub heading: Option<usize>,
    pub code: bool,
    pub comment: bool,
}

pub(crate) const BLOCKER_PREFIX: &str = "<!-- kvist-recovery-blocker:";

pub(crate) fn scan(contents: &str) -> Vec<Line<'_>> {
    let mut fence = None;
    let mut comment = false;
    contents
        .lines()
        .map(|text| {
            let trimmed = text.trim_start_matches(' ');
            let indent = text.len() - trimmed.len();
            let fence_start = trimmed
                .as_bytes()
                .first()
                .copied()
                .filter(|byte| indent <= 3 && matches!(byte, b'`' | b'~'));
            let run = fence_start.map_or(0, |byte| {
                trimmed.bytes().take_while(|next| *next == byte).count()
            });
            let was_fenced = fence.is_some();
            if let Some((byte, length)) = fence {
                if fence_start == Some(byte) && run >= length && trimmed[run..].trim().is_empty() {
                    fence = None;
                }
            } else if !comment
                && run >= 3
                && !(fence_start == Some(b'`') && trimmed[run..].contains('`'))
            {
                fence = fence_start.map(|byte| (byte, run));
            }
            let code = was_fenced || fence.is_some();
            let was_comment = comment;
            if !code {
                let mut remaining = text;
                loop {
                    let delimiter = if comment { "-->" } else { "<!--" };
                    let Some(index) = remaining.find(delimiter) else {
                        break;
                    };
                    remaining = &remaining[index + delimiter.len()..];
                    comment = !comment;
                }
            }
            let heading =
                if !code && !was_comment && !text.trim_start().starts_with("<!--") && indent <= 3 {
                    let level = trimmed.bytes().take_while(|byte| *byte == b'#').count();
                    (level > 0
                        && level <= 6
                        && trimmed[level..]
                            .chars()
                            .next()
                            .is_none_or(char::is_whitespace))
                    .then_some(level)
                } else {
                    None
                };
            Line {
                text,
                heading,
                code,
                comment: was_comment || text.trim_start().starts_with("<!--"),
            }
        })
        .collect()
}

pub(crate) fn section_has_content(lines: &[Line<'_>], index: usize) -> bool {
    let level = lines.get(index).and_then(|line| line.heading).unwrap_or(2);
    lines
        .iter()
        .skip(index + 1)
        .take_while(|line| line.heading.is_none_or(|heading| heading > level))
        .any(|line| !line.text.trim().is_empty() && line.heading.is_none() && !line.comment)
}

pub(crate) fn blockers(contents: &str) -> Vec<String> {
    scan(contents)
        .iter()
        .filter(|line| !line.code)
        .filter_map(|line| line.text.trim().strip_prefix(BLOCKER_PREFIX))
        .map(|reason| reason.trim().trim_end_matches("-->").trim().to_owned())
        .collect()
}

pub(crate) fn draft(reason: &str) -> String {
    format!(
        "{BLOCKER_PREFIX} {reason} -->\nRecovery draft: {reason}. Resolve this placeholder before accepting intent or executing tasks.\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comment_close_and_reopen_does_not_expose_example_headings() {
        let lines = scan("<!-- first\nclosed --> <!-- second\n## Hidden\nclosed -->\n## Visible\n");
        let headings = lines
            .iter()
            .filter(|line| line.heading.is_some())
            .map(|line| line.text)
            .collect::<Vec<_>>();
        assert_eq!(headings, ["## Visible"]);
    }

    #[test]
    fn longer_and_tilde_fences_keep_heading_and_blocker_examples_inert() {
        for fence in ["````", "~~~"] {
            let contents = format!(
                "{fence}markdown\n```\n## Example\n{BLOCKER_PREFIX} inert -->\n{fence}\n## Real\n{BLOCKER_PREFIX} active -->\n"
            );
            let lines = scan(&contents);
            assert_eq!(
                lines
                    .iter()
                    .filter_map(|line| line.heading)
                    .collect::<Vec<_>>(),
                [2]
            );
            assert_eq!(blockers(&contents), ["active"]);
        }
    }
}
