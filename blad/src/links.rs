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

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

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

/// A clickable link found in the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// The display text of the link.
    pub text: String,
    /// The destination URL, fragment or path.
    pub destination: String,
    /// How to follow the destination.
    pub kind: LinkKind,
}

impl Link {
    /// Classify a raw destination and build the link.
    ///
    /// The classification is total: every destination is exactly one of
    /// [`LinkKind::Anchor`], [`LinkKind::Url`] or [`LinkKind::File`].
    pub fn new(text: String, destination: String) -> Self {
        let kind = classify_destination(&destination);
        Link {
            text,
            destination,
            kind,
        }
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
    let parser = Parser::new(source);
    let mut links = Vec::new();
    let mut current_text = String::new();
    let mut in_link = false;
    let mut link_destination = String::new();

    for event in parser {
        match event {
            Event::Start(Tag::Link { dest_url, .. }) => {
                in_link = true;
                link_destination = dest_url.to_string();
                current_text.clear();
            }
            Event::End(TagEnd::Link) => {
                if in_link && !current_text.is_empty() {
                    links.push(Link::new(current_text.clone(), link_destination.clone()));
                }
                in_link = false;
                link_destination.clear();
            }
            // Capture every inline payload inside the label: plain text,
            // inline code and raw inline HTML.
            Event::Text(text) | Event::Code(text) if in_link => {
                current_text.push_str(&text);
            }
            Event::InlineHtml(html) if in_link => {
                current_text.push_str(&html);
            }
            // Emphasis and other inline containers carry their own Text/Code
            // children, which the arms above already capture.
            _ => {}
        }
    }

    links
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
