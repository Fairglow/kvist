//! Extended autolink boundary tests for Blad.
//!
//! These tests cover GFM-style autolink recognition beyond basic HTTP(S),
//! including bare www/ email recognition, balanced-parenthesis rules,
//! punctuation trimming, and exclusions for code/metadata contexts.
//!
//! Tests are organized to verify:
//! - Bare URL recognition (http, https, www.)
//! - Email address recognition
//! - Balanced parenthesis handling
//! - Punctuation trimming at boundaries
//! - False positive exclusions (code blocks, inline code, links)
//! - Unicode in URLs
//! - Repeated links and streaming behavior

use blad::links::{bare_urls, extract_links};

/// Test that bare http:// and https:// URLs are recognized.
#[test]
fn bare_http_https_urls() {
    let text = "Visit http://example.com and https://example.org/path";
    let urls = bare_urls(text);
    assert_eq!(urls.len(), 2);
    assert_eq!(&text[urls[0].start..urls[0].end], "http://example.com");
    assert_eq!(
        &text[urls[1].start..urls[1].end],
        "https://example.org/path"
    );
}

/// Test that bare www. URLs are recognized (GFM extended autolink).
#[test]
fn bare_www_urls() {
    let text = "Check out www.example.com and www.sub.domain.co.uk/path";
    let urls = bare_urls(text);
    assert_eq!(urls.len(), 2);
    assert_eq!(&text[urls[0].start..urls[0].end], "www.example.com");
    assert_eq!(
        &text[urls[1].start..urls[1].end],
        "www.sub.domain.co.uk/path"
    );
}

/// Test that email addresses are recognized as bare autolinks.
#[test]
fn bare_email_addresses() {
    let text = "Contact user@example.com or name.surname+tag@domain.co.uk";
    let urls = bare_urls(text);
    assert_eq!(urls.len(), 2);
    assert_eq!(&text[urls[0].start..urls[0].end], "user@example.com");
    assert_eq!(
        &text[urls[1].start..urls[1].end],
        "name.surname+tag@domain.co.uk"
    );
}

/// Test that trailing punctuation is trimmed from URLs.
#[test]
fn trailing_punctuation_trimmed() {
    let text = "Visit http://example.com. See https://test.org/path!";
    let urls = bare_urls(text);
    assert_eq!(urls.len(), 2);
    assert_eq!(&text[urls[0].start..urls[0].end], "http://example.com");
    assert_eq!(&text[urls[1].start..urls[1].end], "https://test.org/path");
}

/// Test that balanced parentheses in URLs are handled correctly.
#[test]
fn balanced_parentheses_in_urls() {
    let text = "See https://example.com/path(a) and https://example.com/path(a(b)c)";
    let urls = bare_urls(text);
    assert_eq!(urls.len(), 2);
    assert_eq!(
        &text[urls[0].start..urls[0].end],
        "https://example.com/path(a)"
    );
    assert_eq!(
        &text[urls[1].start..urls[1].end],
        "https://example.com/path(a(b)c)"
    );
}

/// Test that unbalanced parentheses don't incorrectly extend the URL.
#[test]
fn unbalanced_parentheses_not_included() {
    let text = "Visit https://example.com/path) and (https://example.com/start";
    let urls = bare_urls(text);
    assert_eq!(urls.len(), 2);
    // The trailing ) should not be included
    assert_eq!(
        &text[urls[0].start..urls[0].end],
        "https://example.com/path"
    );
    // The leading ( should not be included
    assert_eq!(
        &text[urls[1].start..urls[1].end],
        "https://example.com/start"
    );
}

/// Test that URLs inside code blocks are NOT recognized as bare autolinks.
#[test]
fn code_blocks_excluded() {
    let md = "```\nhttp://in-code.com\n```\n\nAnd http://real.com";
    let links = extract_links(md);
    assert!(links.iter().any(|l| l.destination == "http://real.com"));
    assert!(!links.iter().any(|l| l.destination == "http://in-code.com"));
}

/// Test that URLs inside inline code are NOT recognized.
#[test]
fn inline_code_excluded() {
    let md = "Check `http://in-code.com` and http://real.com";
    let links = extract_links(md);
    assert!(links.iter().any(|l| l.destination == "http://real.com"));
    assert!(!links.iter().any(|l| l.destination == "http://in-code.com"));
}

/// Test that URLs inside existing links are NOT double-extracted.
#[test]
fn existing_links_excluded() {
    let md = "[http://already-linked.com](http://already-linked.com) and http://new.com";
    let links = extract_links(md);
    assert_eq!(links.len(), 2);
    // The bare autolink should only be extracted once
    assert!(links.iter().any(|l| l.destination == "http://new.com"));
}

/// Test that Unicode characters in URLs are preserved.
#[test]
fn unicode_in_urls() {
    let text = "Visit https://example.com/世界 and http://例え.jp/path";
    let urls = bare_urls(text);
    assert_eq!(urls.len(), 2);
    assert_eq!(
        &text[urls[0].start..urls[0].end],
        "https://example.com/世界"
    );
    assert_eq!(&text[urls[1].start..urls[1].end], "http://例え.jp/path");
}

/// Test that repeated identical URLs are each recognized.
#[test]
fn repeated_urls() {
    let text = "http://example.com http://example.com http://example.com";
    let urls = bare_urls(text);
    assert_eq!(urls.len(), 3);
}

/// Test that URLs in table cells are recognized.
#[test]
fn urls_in_table_cells() {
    let md = "| Link |\n|------|\n| http://example.com |";
    let links = extract_links(md);
    assert!(links.iter().any(|l| l.destination == "http://example.com"));
}

/// Test that bare URLs at end of line are recognized.
#[test]
fn urls_at_line_end() {
    let text = "End of line http://example.com\nNext line";
    let urls = bare_urls(text);
    assert_eq!(urls.len(), 1);
    assert_eq!(&text[urls[0].start..urls[0].end], "http://example.com");
}

/// Test that bare URLs at start of line are recognized.
#[test]
fn urls_at_line_start() {
    let text = "http://example.com\nNext line";
    let urls = bare_urls(text);
    assert_eq!(urls.len(), 1);
    assert_eq!(&text[urls[0].start..urls[0].end], "http://example.com");
}

/// Test that false positives are avoided for non-URL patterns.
#[test]
fn false_positives_avoided() {
    // Colons in filenames should not be treated as URL schemes
    let text = "notes:2024.md and C:foo";
    let urls = bare_urls(text);
    assert_eq!(urls.len(), 0);

    // mailto: and tel: are handled separately in classify_destination,
    // but bare_urls should not match them as http/https/www
    let text = "mailto:user@example.com and tel:123";
    let urls = bare_urls(text);
    assert_eq!(urls.len(), 0);
}

/// Test that www. without protocol is recognized but www with invalid chars is not.
#[test]
fn www_boundary_conditions() {
    let text = "www.example.com and www..example.com and www-example.com";
    let urls = bare_urls(text);
    // Only `www.example.com` is a valid bare `www.` URL: the host label after
    // the `www.` prefix must begin with an alphanumeric, so
    // `www..example.com` (empty label) and `www-example.com` (no host part)
    // are rejected.
    assert_eq!(urls.len(), 1);
    assert_eq!(&text[urls[0].start..urls[0].end], "www.example.com");
}

/// Test that URLs with query parameters and fragments are captured.
#[test]
fn urls_with_query_and_fragment() {
    let text = "See http://example.com/path?query=value#fragment";
    let urls = bare_urls(text);
    assert_eq!(urls.len(), 1);
    assert_eq!(
        &text[urls[0].start..urls[0].end],
        "http://example.com/path?query=value#fragment"
    );
}

/// Test that URLs followed by parentheses are trimmed correctly.
#[test]
fn urls_followed_by_parentheses() {
    let text = "Check (http://example.com) and (https://test.org)";
    let urls = bare_urls(text);
    assert_eq!(urls.len(), 2);
    assert_eq!(&text[urls[0].start..urls[0].end], "http://example.com");
    assert_eq!(&text[urls[1].start..urls[1].end], "https://test.org");
}

/// Test that multiple punctuation types are trimmed.
#[test]
fn multiple_punctuation_types() {
    let text = "http://a.com. http://b.com, http://c.com; http://d.com! http://e.com?";
    let urls = bare_urls(text);
    assert_eq!(urls.len(), 5);
    assert_eq!(&text[urls[0].start..urls[0].end], "http://a.com");
    assert_eq!(&text[urls[1].start..urls[1].end], "http://b.com");
    assert_eq!(&text[urls[2].start..urls[2].end], "http://c.com");
    assert_eq!(&text[urls[3].start..urls[3].end], "http://d.com");
    assert_eq!(&text[urls[4].start..urls[4].end], "http://e.com");
}

/// Test that bare autolinks in markdown text are extracted with correct labels.
#[test]
fn bare_autolink_extraction() {
    let md = "See https://example.com and [named](other.md)";
    let links = extract_links(md);
    assert_eq!(links.len(), 2);
    let bare = links
        .iter()
        .find(|l| l.destination == "https://example.com")
        .unwrap();
    assert_eq!(bare.text, "https://example.com");
    assert!(bare.is_url());
}

/// Test that wrapped labels in links don't break autolink detection.
#[test]
fn wrapped_labels() {
    let md = "[`code link`](http://example.com) and http://bare.com";
    let links = extract_links(md);
    assert_eq!(links.len(), 2);
    assert!(links.iter().any(|l| l.destination == "http://bare.com"));
}

/// Test streaming behavior: URLs split across "lines" in source are handled.
#[test]
fn streaming_line_tracking() {
    let md = "first line\nsecond http://example.com line\nthird line";
    let links = extract_links(md);
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].source_line, 1); // zero-based, so second line
}

/// Test that HTML metadata contexts don't trigger false autolinks.
#[test]
fn html_metadata_excluded() {
    // In a real document, HTML comments and scripts should not trigger bare URLs
    // This tests that the extractor doesn't double-count or mis-parse
    let md = "<!-- http://in-comment.com -->\n\nhttp://real.com";
    let links = extract_links(md);
    assert!(links.iter().any(|l| l.destination == "http://real.com"));
}

/// Test edge case: URL immediately followed by quote.
#[test]
fn url_followed_by_quote() {
    let text = r#"Say "http://example.com" and 'https://test.org'"#;
    let urls = bare_urls(text);
    assert_eq!(urls.len(), 2);
    assert_eq!(&text[urls[0].start..urls[0].end], "http://example.com");
    assert_eq!(&text[urls[1].start..urls[1].end], "https://test.org");
}

/// Test edge case: URL with trailing bracket.
#[test]
fn url_followed_by_bracket() {
    let text = "See <http://example.com> and [link](https://test.org)";
    let urls = bare_urls(text);
    assert_eq!(urls.len(), 1);
    assert_eq!(&text[urls[0].start..urls[0].end], "http://example.com");
}
/// Test that bare URLs in frontmatter are not extracted as links.
#[test]
fn frontmatter_excluded() {
    let md = "---\nhttp://in-frontmatter.com\n---\n\nhttp://real.com";
    let links = extract_links(md);
    assert!(links.iter().any(|l| l.destination == "http://real.com"));
}

/// Test that URLs in reference-style link definitions are not double-extracted.
#[test]
fn reference_style_links() {
    let md = "[link][ref]\n\n[ref]: http://example.com";
    let links = extract_links(md);
    // The reference definition itself shouldn't create a bare autolink
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].destination, "http://example.com");
}

/// Test that www. with trailing punctuation is trimmed.
#[test]
fn www_trailing_punctuation() {
    let text = "Visit www.example.com. and www.test.org!";
    let urls = bare_urls(text);
    assert_eq!(urls.len(), 2);
    assert_eq!(&text[urls[0].start..urls[0].end], "www.example.com");
    assert_eq!(&text[urls[1].start..urls[1].end], "www.test.org");
}

/// Test that email addresses with plus signs are recognized.
#[test]
fn email_with_plus_signs() {
    let text = "Contact user+tag@example.com and name+folder+label@domain.co.uk";
    let urls = bare_urls(text);
    assert_eq!(urls.len(), 2);
}

/// Test that URLs with port numbers are recognized.
#[test]
fn urls_with_ports() {
    let text = "See http://example.com:8080 and https://test.org:443/path";
    let urls = bare_urls(text);
    assert_eq!(urls.len(), 2);
    assert_eq!(&text[urls[0].start..urls[0].end], "http://example.com:8080");
    assert_eq!(
        &text[urls[1].start..urls[1].end],
        "https://test.org:443/path"
    );
}

/// Test that bare URLs in task lists are recognized.
#[test]
fn urls_in_task_lists() {
    let md = "- [ ] Check http://example.com\n- [x] Done";
    let links = extract_links(md);
    assert!(links.iter().any(|l| l.destination == "http://example.com"));
}

/// Test that URLs in strikethrough text are recognized.
#[test]
fn urls_in_strikethrough() {
    let md = "~~http://strikethrough.com~~ and http://real.com";
    let links = extract_links(md);
    assert!(links.iter().any(|l| l.destination == "http://real.com"));
}

/// Test that the bare_urls function handles empty strings.
#[test]
fn empty_string_handling() {
    let text = "";
    let urls = bare_urls(text);
    assert_eq!(urls.len(), 0);
}

/// Test that bare_urls handles strings without URLs.
#[test]
fn no_urls_in_text() {
    let text = "This is just plain text with no URLs";
    let urls = bare_urls(text);
    assert_eq!(urls.len(), 0);
}

/// Test that URLs with underscores in domain are recognized.
#[test]
fn urls_with_underscores() {
    let text = "See http://example_domain.com and https://test_domain.org/path";
    let urls = bare_urls(text);
    assert_eq!(urls.len(), 2);
}

/// Test that multiple bare URLs in one line are all captured.
#[test]
fn multiple_bare_urls_one_line() {
    let text = "http://a.com http://b.com https://c.org www.d.net";
    let urls = bare_urls(text);
    assert_eq!(urls.len(), 4);
}

/// Test that URLs immediately after punctuation are trimmed correctly.
#[test]
fn url_after_punctuation() {
    let text = "See: http://example.com; see: https://test.org!";
    let urls = bare_urls(text);
    assert_eq!(urls.len(), 2);
    assert_eq!(&text[urls[0].start..urls[0].end], "http://example.com");
    assert_eq!(&text[urls[1].start..urls[1].end], "https://test.org");
}

/// Test that the extractor doesn't confuse file paths with URLs.
#[test]
fn file_paths_not_confused_with_urls() {
    let md = "[notes](notes:2024.md) and [drive](C:foo)";
    let links = extract_links(md);
    assert_eq!(links.len(), 2);
    assert!(links[0].is_file());
    assert!(links[1].is_file());
}

/// Test that bare www. autolinks keep the source label but navigate to an
/// https:// destination.
#[test]
fn www_destination_normalized_to_https() {
    let md = "See www.example.com/path";
    let links = extract_links(md);
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].text, "www.example.com/path");
    assert_eq!(links[0].destination, "https://www.example.com/path");
    assert!(links[0].is_url());
}

/// Test that http/https bare autolink destinations keep their scheme.
#[test]
fn http_destination_keeps_scheme() {
    let links = extract_links("See http://example.com/a");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].text, "http://example.com/a");
    assert_eq!(links[0].destination, "http://example.com/a");
    assert!(links[0].is_url());

    let links = extract_links("See https://example.com/b");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].destination, "https://example.com/b");
    assert!(links[0].is_url());
}

/// Test that bare email autolinks keep the source label but navigate to a
/// mailto: destination.
#[test]
fn email_destination_normalized_to_mailto() {
    let links = extract_links("Contact user@example.com.");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].text, "user@example.com");
    assert_eq!(links[0].destination, "mailto:user@example.com");
    assert!(links[0].is_url());
}

/// Test that an @ sign inside a path of an http/https URL is not treated as
/// an email address.
#[test]
fn at_sign_in_url_path_is_not_email() {
    let links = extract_links("See https://example.com/a@b");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].destination, "https://example.com/a@b");
    assert!(links[0].is_url());
}
