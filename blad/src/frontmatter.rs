use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

pub(crate) fn events(
    source: &str,
    options: Options,
) -> impl Iterator<Item = (Event<'_>, Range<usize>)> {
    let header = prefix(source);
    let offset = header.as_ref().map_or(0, |range| range.end);
    // Code events share literal rendering and navigation exclusion. The native
    // metadata extension also admits non-prefix blocks and rejects empty ones.
    let metadata = header.into_iter().flat_map(|range| {
        [
            (
                Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced("yaml".into()))),
                range.clone(),
            ),
            (Event::Text(source[range.clone()].into()), range.clone()),
            (Event::End(TagEnd::CodeBlock), range),
        ]
    });
    metadata.chain(
        Parser::new_ext(&source[offset..], options)
            .into_offset_iter()
            .map(move |(event, range)| (event, range.start + offset..range.end + offset)),
    )
}

fn prefix(source: &str) -> Option<Range<usize>> {
    let start = if source.starts_with('\u{feff}') {
        '\u{feff}'.len_utf8()
    } else {
        0
    };
    let mut lines = source[start..].split_inclusive('\n');
    let opening = lines.next()?;
    if delimiter(opening) != "---" {
        return None;
    }
    let mut end = start + opening.len();
    for line in lines {
        end += line.len();
        if matches!(delimiter(line), "---" | "...") {
            return Some(start..end);
        }
    }
    None
}

fn delimiter(line: &str) -> &str {
    let line = line
        .strip_suffix('\n')
        .map_or(line, |line| line.strip_suffix('\r').unwrap_or(line));
    line.trim_end_matches([' ', '\t'])
}
