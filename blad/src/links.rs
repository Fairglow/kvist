//! Link extraction and navigation for Blad.
//!
//! Parses Markdown source to extract clickable links and internal anchors,
//! enabling Tab/Shift+Tab navigation and Enter to follow.
//!
//! The extractor recognises the same inline link syntax the renderer uses
//! (autolinks, reference-style and inline links) and captures *all* inline
//! content in the link label, including inline code and emphasis, so a link
//! such as ``[`foo.rs`](foo.rs)`` is not silently dropped.

use std::path::{Path, PathBuf};

use pulldown_cmark::{Event, Options, Tag, TagEnd};

/// The class of a link destination, used to decide how to follow it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    /// An internal fragment link such as `#section`.
    Anchor,
    /// A scheme URL such as `https://`, `mailto:` or `tel:`.
    Url,
    /// A relative or absolute file path.
    File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum NavigationId {
    Source(usize),
    Bare { event: usize, offset: usize },
}

/// A clickable link found in the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// The display text of the link.
    pub text: String,
    /// The destination URL, fragment or path.
    pub destination: String,
    /// How to follow the destination.
    pub kind: LinkKind,
    /// The zero-based source line the link begins on, used to map a link to
    /// its rendered line for cursor-following.
    pub source_line: usize,
    /// Parser source offset used by renderer-owned navigation metadata.
    pub source_offset: usize,
    bare_offset: Option<usize>,
}

impl Link {
    /// Classify a raw destination and build the link on line `source_line`.
    ///
    /// The classification is total: every destination is exactly one of
    /// [`LinkKind::Anchor`], [`LinkKind::Url`] or [`LinkKind::File`].
    pub fn new(text: String, destination: String) -> Self {
        let kind = classify_destination(&destination);
        Link {
            text,
            destination,
            kind,
            source_line: 0,
            source_offset: 0,
            bare_offset: None,
        }
    }

    /// Record the source line a link begins on.
    pub fn with_source_line(mut self, line: usize) -> Self {
        self.source_line = line;
        self
    }

    /// True when the destination is an internal `#fragment` link.
    pub fn is_anchor(&self) -> bool {
        self.kind == LinkKind::Anchor
    }

    /// True when the destination is a scheme URL opened in a browser.
    pub fn is_url(&self) -> bool {
        self.kind == LinkKind::Url
    }

    /// True when the destination is a file path.
    pub fn is_file(&self) -> bool {
        self.kind == LinkKind::File
    }

    pub(crate) fn navigation_id(&self) -> NavigationId {
        self.bare_offset
            .map_or(NavigationId::Source(self.source_offset), |offset| {
                NavigationId::Bare {
                    event: self.source_offset,
                    offset,
                }
            })
    }
}

/// Classify a raw link destination.
///
/// A leading `#` is an internal anchor. A destination that starts with a URI
/// scheme followed by `://` (e.g. `https://`, `ftp://`, `file:///`) is a URL,
/// as are the common non-hierarchical schemes `mailto:` and `tel:`. Anything
/// else is a file path — in particular a Linux filename that contains a colon
/// such as `notes:2024.md` is not mistaken for a URL.
fn classify_destination(destination: &str) -> LinkKind {
    if destination.starts_with('#') {
        return LinkKind::Anchor;
    }
    if destination.starts_with("mailto:") || destination.starts_with("tel:") {
        return LinkKind::Url;
    }
    if let Some(scheme_end) = destination.find("://") {
        let scheme = &destination[..scheme_end];
        if !scheme.is_empty()
            && scheme
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'-' || b == b'.')
        {
            return LinkKind::Url;
        }
    }
    LinkKind::File
}

/// Resolve a file link against the directory of the document it appears in.
///
/// A `#fragment` suffix is stripped. A destination that is already absolute is
/// returned as-is; a relative destination is joined onto `base` (the directory
/// of the current file) when one is known, otherwise it is returned unchanged.
/// The result is not canonicalised or symlink-resolved: the viewer is the
/// user's own process, and the user explicitly chose to follow the link.
pub fn resolve_file(base: Option<&Path>, destination: &str) -> PathBuf {
    let path = destination.split('#').next().unwrap_or("");
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        return candidate.to_path_buf();
    }
    match base {
        Some(dir) => dir.join(candidate),
        None => candidate.to_path_buf(),
    }
}

/// Extract all clickable links from Markdown source.
///
/// Inline code and emphasis inside a link label are captured as text, so the
/// label of ``[`foo.rs`](foo.rs)`` is `foo.rs` rather than an empty string.
pub fn extract_links(source: &str) -> Vec<Link> {
    let parser = crate::frontmatter::events(source, parser_options());
    let mut links = Vec::new();
    let mut current_text = String::new();
    let mut in_link = false;
    let mut in_code = false;
    let mut link_destination = String::new();
    let mut link_line = 0;
    let mut link_offset = 0;

    for (event, range) in crate::scripts::events(source, parser) {
        match event {
            Event::Start(Tag::Link { dest_url, .. }) => {
                in_link = true;
                link_destination = dest_url.to_string();
                current_text.clear();
                link_line = line_of(source, range.start);
                link_offset = range.start;
            }
            Event::End(TagEnd::Link) => {
                if in_link && !current_text.is_empty() {
                    let mut link = Link::new(current_text.clone(), link_destination.clone())
                        .with_source_line(link_line);
                    link.source_offset = link_offset;
                    links.push(link);
                }
                in_link = false;
                link_destination.clear();
            }
            Event::Start(Tag::CodeBlock(_)) => in_code = true,
            Event::End(TagEnd::CodeBlock) => in_code = false,
            // Capture every inline payload inside the label: plain text,
            // inline code and raw inline HTML.
            Event::Text(text)
            | Event::Code(text)
            | Event::InlineMath(text)
            | Event::DisplayMath(text)
            | Event::FootnoteReference(text)
                if in_link =>
            {
                current_text.push_str(&text);
            }
            Event::SoftBreak | Event::HardBreak if in_link => current_text.push(' '),
            Event::InlineHtml(html) if in_link => {
                current_text.push_str(&html);
            }
            // Bare autolinks: a plain-text run that is not already inside a
            // link or a code block may contain a bare `http(s)://` URL.
            Event::Text(text) if !in_link && !in_code => {
                let projected = crate::scripts::text(&text, source, range.clone());
                let text = crate::terminal_text::inert_source(&projected);
                for url in bare_urls(&text) {
                    let mut link = Link::new(text[url.clone()].into(), text[url.clone()].into())
                        .with_source_line(line_of(source, range.start));
                    link.source_offset = range.start;
                    link.bare_offset = Some(url.start);
                    links.push(link);
                }
            }
            // Emphasis and other inline containers carry their own Text/Code
            // children, which the arms above already capture.
            _ => {}
        }
    }

    links
}

/// The zero-based line containing byte offset `pos`.
fn line_of(source: &str, pos: usize) -> usize {
    source[..pos.min(source.len())].matches('\n').count()
}

/// Find bare `http://` and `https://` URLs in a plain-text run, trimming
/// trailing punctuation that is not part of the URL.
pub(crate) fn bare_urls(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut urls = Vec::new();
    let mut i = 0;
    while i < text.len() {
        if text.as_bytes()[i..].starts_with(b"https://")
            || text.as_bytes()[i..].starts_with(b"http://")
        {
            let start = i;
            let mut end = i;
            while end < text.len() {
                let c = text.as_bytes()[end];
                if c.is_ascii_whitespace() || matches!(c, b'"' | b'\'' | b'<' | b'>' | b'(' | b')')
                {
                    break;
                }
                end += 1;
            }
            // Trim trailing sentence punctuation that is not part of the URL.
            while end > start {
                let c = text.as_bytes()[end - 1];
                if matches!(c, b'.' | b',' | b';' | b'!' | b'?' | b':') {
                    end -= 1;
                } else {
                    break;
                }
            }
            if end > start {
                urls.push(start..end);
            }
            i = end;
        } else {
            i += 1;
        }
    }
    urls
}

pub(crate) fn parser_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_MATH
        | Options::ENABLE_SUPERSCRIPT
        | Options::ENABLE_SUBSCRIPT
        | Options::ENABLE_GFM
}

pub(crate) fn heading_anchors(source: &str) -> std::collections::BTreeMap<usize, Vec<String>> {
    let mut anchors = std::collections::BTreeMap::new();
    let mut used = std::collections::BTreeSet::new();
    let mut suffixes = std::collections::BTreeMap::<String, usize>::new();
    let mut heading = None;
    let mut label = String::new();
    for (event, range) in crate::frontmatter::events(source, parser_options()) {
        match event {
            Event::Start(Tag::Heading { id, .. }) => {
                heading = Some((range.start, id.map(|id| id.to_string())));
                label.clear();
            }
            Event::Text(text) | Event::Code(text) if heading.is_some() => label.push_str(&text),
            Event::SoftBreak | Event::HardBreak if heading.is_some() => label.push(' '),
            Event::End(TagEnd::Heading(_)) => {
                if let Some((offset, explicit)) = heading.take() {
                    let base: String = label
                        .to_lowercase()
                        .chars()
                        .filter_map(|ch| {
                            if ch.is_whitespace() {
                                Some('-')
                            } else if ch.is_alphanumeric() || matches!(ch, '-' | '_') {
                                Some(ch)
                            } else {
                                None
                            }
                        })
                        .collect();
                    let mut id = base.clone();
                    let suffix = suffixes.entry(base.clone()).or_default();
                    while used.contains(&id) {
                        *suffix += 1;
                        id = format!("{base}-{suffix}");
                    }
                    used.insert(id.clone());
                    let mut ids = vec![id];
                    if let Some(explicit) = explicit {
                        used.insert(explicit.clone());
                        ids.push(explicit);
                    }
                    anchors.insert(offset, ids);
                }
            }
            _ => {}
        }
    }
    anchors
}

pub(crate) fn decode_destination(value: &str) -> std::io::Result<String> {
    let mut decoded = Vec::with_capacity(value.len());
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let high = bytes.next().and_then(|ch| (ch as char).to_digit(16));
            let low = bytes.next().and_then(|ch| (ch as char).to_digit(16));
            let (Some(high), Some(low)) = (high, low) else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "invalid percent encoding in link",
                ));
            };
            decoded.push((high * 16 + low) as u8);
        } else {
            decoded.push(byte);
        }
    }
    String::from_utf8(decoded)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_bare_urls_and_named_references_are_extracted() {
        let links = extract_links(
            "世界 https://example.com/世界 and [GitHub][gh]\n\n[gh]: https://github.com",
        );
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].destination, "https://example.com/世界");
        assert_eq!(links[1].text, "GitHub");
    }

    #[test]
    fn heading_ids_disambiguate_duplicates_and_preserve_unicode() {
        let anchors = heading_anchors("# Hello, 世界!\n\n## Hello, 世界!\n\n## Explicit {#custom}");
        let ids: Vec<_> = anchors.values().flatten().map(String::as_str).collect();
        assert_eq!(ids, ["hello-世界", "hello-世界-1", "explicit", "custom"]);
    }

    #[test]
    fn destinations_decode_utf8_without_accepting_bad_escapes() {
        assert_eq!(decode_destination("a%20b%23%E4%B8%96").unwrap(), "a b#世");
        for invalid in ["%", "%2", "%GG", "%FF"] {
            assert!(decode_destination(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn extracts_basic_url_link() {
        let links = extract_links("[Click here](https://example.com)");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].text, "Click here");
        assert_eq!(links[0].destination, "https://example.com");
        assert!(links[0].is_url());
    }

    #[test]
    fn extracts_anchor_link() {
        let links = extract_links("[Section](#heading)");
        assert_eq!(links.len(), 1);
        assert!(links[0].is_anchor());
    }

    #[test]
    fn extracts_file_link() {
        let links = extract_links("[Other file](other.md)");
        assert_eq!(links.len(), 1);
        assert!(links[0].is_file());
    }

    #[test]
    fn captures_inline_code_in_link_label() {
        let links = extract_links(r#"[`foo.rs`](foo.rs)"#);
        assert_eq!(links.len(), 1, "pure code-span label must not be dropped");
        assert_eq!(links[0].text, "foo.rs");
        assert!(links[0].is_file());
    }

    #[test]
    fn captures_emphasis_in_link_label() {
        let links = extract_links("[**bold** target](target.md)");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].text, "bold target");
        assert!(links[0].is_file());
    }

    #[test]
    fn drops_empty_label() {
        let links = extract_links("[](foo.md)");
        assert!(links.is_empty());
    }

    #[test]
    fn classifies_scheme_urls() {
        for (dest, kind) in [
            ("https://x", LinkKind::Url),
            ("http://x", LinkKind::Url),
            ("mailto:x@y", LinkKind::Url),
            ("tel:123", LinkKind::Url),
            ("file:///etc/passwd", LinkKind::Url),
            ("#sec", LinkKind::Anchor),
            ("rel.md", LinkKind::File),
            ("../up.md", LinkKind::File),
            ("/abs/path.md", LinkKind::File),
        ] {
            let link = Link::new("t".into(), dest.into());
            assert_eq!(link.kind, kind, "misclassified {dest}");
        }
    }

    #[test]
    fn colon_in_filename_is_not_a_scheme() {
        // `notes:2024.md` is a Linux file path, not a URL: the colon is not
        // followed by `//` and it is not a mailto/tel prefix.
        let link = Link::new("t".into(), "notes:2024.md".into());
        assert_eq!(link.kind, LinkKind::File);
        // A bare `C:foo` is likewise not a URL under the `://` rule.
        let drive = Link::new("t".into(), "C:foo".into());
        assert_eq!(drive.kind, LinkKind::File);
    }

    #[test]
    fn resolves_relative_file_against_base() {
        let base = Path::new("/repo/docs");
        assert_eq!(
            resolve_file(Some(base), "other.md"),
            Path::new("/repo/docs/other.md")
        );
    }

    #[test]
    fn resolves_absolute_file_ignoring_base() {
        let base = Path::new("/repo/docs");
        assert_eq!(
            resolve_file(Some(base), "/etc/hosts"),
            Path::new("/etc/hosts")
        );
    }

    #[test]
    fn strips_fragment_from_file_destination() {
        let base = Path::new("/repo");
        assert_eq!(
            resolve_file(Some(base), "notes.md#section"),
            Path::new("/repo/notes.md")
        );
    }

    #[test]
    fn resolves_without_base() {
        assert_eq!(resolve_file(None, "rel.md"), Path::new("rel.md"));
    }

    #[test]
    fn multiple_links_preserve_document_order() {
        let md = "[a](1.md) text [b](2.md) more [c](#x)";
        let links = extract_links(md);
        assert_eq!(links.len(), 3);
        assert_eq!(links[0].destination, "1.md");
        assert_eq!(links[1].destination, "2.md");
        assert_eq!(links[2].destination, "#x");
    }

    #[test]
    fn extracts_bare_autolinks() {
        let links = extract_links("See https://example.com and [named](other.md)");
        assert_eq!(links.len(), 2);
        assert!(links.iter().any(|l| l.destination == "https://example.com"));
        assert!(links.iter().any(|l| l.text == "named"));
    }

    #[test]
    fn bare_autolink_trims_trailing_punctuation() {
        let links = extract_links("Visit https://example.com/path. Then stop.");
        assert_eq!(links[0].destination, "https://example.com/path");
    }

    #[test]
    fn bare_autolinks_are_not_extracted_from_code_blocks() {
        let links = extract_links("```\nhttps://in-code.com\n```\nand https://real.com");
        assert!(links.iter().any(|l| l.destination == "https://real.com"));
        assert!(!links.iter().any(|l| l.destination == "https://in-code.com"));
    }

    #[test]
    fn link_source_line_tracks_its_line() {
        let links = extract_links("first line\n\nsecond https://x.com line");
        assert_eq!(links[0].source_line, 2);
    }
}
