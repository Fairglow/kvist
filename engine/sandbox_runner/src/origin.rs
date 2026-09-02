//! Source-aware validated origin parsing and matching for mediated acquisition.
//!
//! This module is a reusable, host-independent primitive that the later
//! Bubblewrap acquisition integration will invoke to decide whether a concrete
//! outbound request URL is permitted by the approved package sources. It does
//! **not** perform any network I/O, open any socket, or enforce anything by
//! itself; it only parses untrusted URL strings into a strict canonical
//! [`Origin`] and answers exact scheme/host/port/path-prefix membership
//! questions. Network *enforcement* (namespace setup, an application-aware
//! fetch boundary) is owned by the deferred runner-integration task; keeping
//! the matcher separate lets that task wire an already-tested policy instead of
//! inventing one.
//!
//! The parser is deliberately strict and fail-closed. It rejects credentials
//! (`userinfo`), query strings, fragments, any percent-encoding (which could
//! alias a `/` or `.` and defeat path-prefix checks), `.`/`..` path traversal,
//! duplicate separators, and non-`http(s)` schemes. Plain `http` can be
//! represented only by the explicit test-loopback policy; production request
//! validation calls [`parse_production`] and never accepts it.

use std::{fmt, net::IpAddr};

/// The exact, canonical sparse index origin of the built-in crates.io source.
pub const CANONICAL_CRATES_IO_INDEX_ORIGIN: &str = "https://index.crates.io/";
/// The exact, canonical crate download origin of the built-in crates.io source.
pub const CANONICAL_CRATES_IO_DOWNLOAD_ORIGIN: &str = "https://static.crates.io/";
/// The exact, reserved name of the built-in canonical crates.io source.
pub const CANONICAL_CRATES_IO_NAME: &str = "crates-io";

/// Maximum accepted length of a URL string before parsing.
const MAX_URL_BYTES: usize = 4096;

/// A URL scheme accepted for a package source or outbound request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    /// Transport-secured HTTP. Required for every non-loopback source.
    Https,
    /// Plain HTTP. Accepted only when the host is a loopback literal.
    Http,
}

/// Policy mode for a future application-aware package transport.
///
/// Production policies never accept a plain-HTTP origin or a private address.
/// The loopback mode exists only for isolated test fixtures and is not used by
/// request validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportPolicyMode {
    /// HTTPS package sources whose resolved addresses must be public.
    ProductionRemote,
    /// Controlled loopback HTTP/HTTPS test endpoints.
    TestLoopback,
}

/// Protocol-defined request classes. No untyped query-string escape hatch is
/// exposed; current Cargo source classes require an empty query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestKind {
    /// Sparse-index metadata fetch.
    CargoRegistryIndex,
    /// Crate archive download.
    CargoCrateDownload,
    /// Immutable Git-object transfer.
    CargoGit,
}

impl Scheme {
    fn default_port(self) -> u16 {
        match self {
            Scheme::Https => 443,
            Scheme::Http => 80,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Scheme::Https => "https",
            Scheme::Http => "http",
        }
    }
}

/// A precise reason an untrusted URL was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginError {
    detail: String,
}

impl OriginError {
    fn new(detail: impl Into<String>) -> Self {
        OriginError {
            detail: detail.into(),
        }
    }
}

impl fmt::Display for OriginError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for OriginError {}

/// A strictly parsed, canonical origin: scheme, host, explicit port, and a
/// canonical absolute path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    scheme: Scheme,
    host: String,
    port: u16,
    path: String,
}

impl Origin {
    /// The parsed scheme.
    pub fn scheme(&self) -> Scheme {
        self.scheme
    }

    /// The lowercased host (a registered name or IP literal without brackets).
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The explicit or scheme-default port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The canonical absolute path (always begins with `/`).
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns true when this origin's scheme, host, and port match `other`.
    fn same_authority(&self, other: &Origin) -> bool {
        self.scheme == other.scheme && self.host == other.host && self.port == other.port
    }
}

/// Parses one untrusted URL into a strict canonical [`Origin`].
///
/// The parser rejects credentials, query strings, fragments, percent-encoding,
/// `.`/`..` traversal, duplicate `/` separators, empty hosts, and non-`http(s)`
/// schemes. Plain `http` is accepted only for a loopback host.
pub fn parse(url: &str) -> Result<Origin, OriginError> {
    if url.len() > MAX_URL_BYTES {
        return Err(OriginError::new(format!(
            "source URL of {} bytes exceeds the {MAX_URL_BYTES}-byte limit",
            url.len()
        )));
    }

    if url.contains('\0') {
        return Err(OriginError::new("source URL contains a NUL byte"));
    }
    if url.contains('%') {
        return Err(OriginError::new(
            "source URL must not use percent-encoding, which could alias a path separator or traversal",
        ));
    }
    if url.contains(['?', '#']) {
        return Err(OriginError::new(
            "source URL must not contain a query string or fragment",
        ));
    }
    if url.chars().any(|character| character.is_whitespace()) {
        return Err(OriginError::new("source URL must not contain whitespace"));
    }

    let (scheme, rest) = if let Some(rest) = url.strip_prefix("https://") {
        (Scheme::Https, rest)
    } else if let Some(rest) = url.strip_prefix("http://") {
        (Scheme::Http, rest)
    } else {
        return Err(OriginError::new(
            "source URL must use the `https://` scheme (or `http://` for a loopback host)",
        ));
    };

    let (authority, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };
    if authority.is_empty() {
        return Err(OriginError::new("source URL must declare a host"));
    }
    if authority.contains('@') {
        return Err(OriginError::new(
            "source URL must not embed credentials (userinfo)",
        ));
    }

    let (host, port) = split_authority(authority, scheme)?;
    validate_host(&host)?;

    if scheme == Scheme::Http && !is_loopback_host(&host) {
        return Err(OriginError::new(
            "plain `http` is permitted only for a loopback host; every remote source must use `https`",
        ));
    }

    let canonical_path = canonical_path(path)?;

    Ok(Origin {
        scheme,
        host,
        port,
        path: canonical_path,
    })
}

/// Parses an origin suitable for a production request declaration.
///
/// This is intentionally stricter than [`parse`]: the latter is retained for
/// test-policy construction, while production request validation has no
/// loopback HTTP exception.
pub fn parse_production(url: &str) -> Result<Origin, OriginError> {
    let origin = parse(url)?;
    if origin.scheme != Scheme::Https {
        return Err(OriginError::new(
            "production package-source origins must use https",
        ));
    }
    if let Ok(address) = origin.host.parse::<IpAddr>()
        && !is_public_address(address)
    {
        return Err(OriginError::new(
            "production package-source origins must not use a private, link-local, or loopback address",
        ));
    }
    if origin.host == "localhost" {
        return Err(OriginError::new(
            "production package-source origins must not use localhost",
        ));
    }
    Ok(origin)
}

fn split_authority(authority: &str, scheme: Scheme) -> Result<(String, u16), OriginError> {
    if let Some(rest) = authority.strip_prefix('[') {
        // Bracketed IPv6 literal, optionally followed by `:port`.
        let Some(close) = rest.find(']') else {
            return Err(OriginError::new(
                "source URL IPv6 host is missing a closing bracket",
            ));
        };
        let host = &rest[..close];
        let after = &rest[close + 1..];
        let port = parse_port(after, scheme)?;
        return Ok((host.to_ascii_lowercase(), port));
    }
    match authority.rsplit_once(':') {
        Some((host, port)) => Ok((host.to_ascii_lowercase(), parse_explicit_port(port)?)),
        None => Ok((authority.to_ascii_lowercase(), scheme.default_port())),
    }
}

fn parse_port(after_bracket: &str, scheme: Scheme) -> Result<u16, OriginError> {
    if after_bracket.is_empty() {
        return Ok(scheme.default_port());
    }
    let Some(port) = after_bracket.strip_prefix(':') else {
        return Err(OriginError::new(
            "source URL has unexpected characters after the IPv6 host",
        ));
    };
    parse_explicit_port(port)
}

fn parse_explicit_port(port: &str) -> Result<u16, OriginError> {
    if port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(OriginError::new("source URL port must be a decimal number"));
    }
    port.parse::<u16>()
        .map_err(|_| OriginError::new("source URL port is out of range"))
}

fn validate_host(host: &str) -> Result<(), OriginError> {
    if host.is_empty() {
        return Err(OriginError::new("source URL host must not be empty"));
    }
    // Registered names, IPv4 literals, and unbracketed IPv6 literals. Reject
    // anything with a path separator or characters that are not host-legal.
    let host_legal = host
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b':'));
    if !host_legal {
        return Err(OriginError::new(
            "source URL host contains characters that are not host-legal",
        ));
    }
    if host.contains(':') {
        // Only a real IPv6 literal (which reached here in bracketed form) may
        // contain a colon; reject a malformed one so it cannot masquerade as a
        // registered name.
        if host.parse::<std::net::Ipv6Addr>().is_err() {
            return Err(OriginError::new(
                "source URL IPv6 host is not a valid address literal",
            ));
        }
        return Ok(());
    }
    if host.starts_with('.') || host.ends_with('.') || host.contains("..") {
        return Err(OriginError::new(
            "source URL host must not have an empty label",
        ));
    }
    if is_numeric_ipv4_alias(host) {
        return Err(OriginError::new(
            "source URL host must not use a noncanonical numeric IPv4 alias (single integer, shortened, octal, or hexadecimal)",
        ));
    }
    Ok(())
}

/// Returns true when a host looks like an intended numeric IPv4 literal but is
/// not written in the single canonical dotted-decimal form. Aliases such as a
/// bare integer (`2130706433`), a shortened form (`127.1`), an octal label
/// (`0177.0.0.1`), or a hexadecimal form (`0x7f.0.0.1`) resolve to the same
/// address as `127.0.0.1` through `inet_aton`-style parsing but bypass the
/// dotted-decimal IP-literal classification, so they are rejected outright.
fn is_numeric_ipv4_alias(host: &str) -> bool {
    if is_canonical_dotted_ipv4(host) {
        return false;
    }
    let has_digit = host.bytes().any(|byte| byte.is_ascii_digit());
    // A host composed solely of hexadecimal digits, `.`, and the `0x` marker
    // (and containing at least one digit) is an IP-literal spelling attempt.
    // Any genuine registered name contains a non-hexadecimal letter.
    has_digit
        && host
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() || matches!(byte, b'.' | b'x' | b'X'))
}

/// Returns true only for the single canonical dotted-decimal IPv4 spelling:
/// exactly four decimal octets in `0..=255` with no leading zeros.
fn is_canonical_dotted_ipv4(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() == 4
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.len() <= 3
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && !(part.len() > 1 && part.starts_with('0'))
                && part.parse::<u8>().is_ok()
        })
}

fn is_loopback_host(host: &str) -> bool {
    if host == "localhost" || host == "::1" {
        return true;
    }
    // IPv4 loopback range 127.0.0.0/8.
    let octets: Vec<&str> = host.split('.').collect();
    if octets.len() == 4 && octets.iter().all(|octet| octet.parse::<u8>().is_ok()) {
        return octets[0] == "127";
    }
    false
}

/// Canonicalizes and validates the path portion of a URL.
fn canonical_path(path: &str) -> Result<String, OriginError> {
    if path.is_empty() {
        return Ok("/".to_owned());
    }
    if !path.starts_with('/') {
        return Err(OriginError::new("source URL path must be absolute"));
    }
    // Preserve a single trailing slash as a distinct canonical form (an origin
    // prefix such as `/index/` differs from an exact resource `/index`).
    let trailing = path.len() > 1 && path.ends_with('/');
    let mut components = Vec::new();
    for component in path.split('/') {
        match component {
            "" => {}
            "." | ".." => {
                return Err(OriginError::new(
                    "source URL path must be canonical without `.` or `..` components",
                ));
            }
            other => components.push(other),
        }
    }
    if path.contains("//") {
        return Err(OriginError::new(
            "source URL path must be canonical without duplicate `/` separators",
        ));
    }
    let mut canonical = format!("/{}", components.join("/"));
    if trailing && canonical != "/" {
        canonical.push('/');
    }
    Ok(canonical)
}

/// A validated set of allowed origins used to decide whether a concrete request
/// URL is permitted. Membership requires an exact scheme/host/port match and a
/// path-prefix containment against one declared origin.
#[derive(Debug, Clone)]
pub struct OriginPolicy {
    allowed: Vec<Origin>,
    mode: TransportPolicyMode,
}

impl Default for OriginPolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl OriginPolicy {
    /// Builds an empty policy that permits nothing.
    pub fn new() -> Self {
        OriginPolicy {
            allowed: Vec::new(),
            mode: TransportPolicyMode::ProductionRemote,
        }
    }

    /// Builds the explicit test-only policy that can represent loopback
    /// endpoints. Production request validation never constructs this mode.
    pub fn test_loopback() -> Self {
        OriginPolicy {
            allowed: Vec::new(),
            mode: TransportPolicyMode::TestLoopback,
        }
    }

    /// Adds one already-parsed origin only if it satisfies this policy mode.
    pub fn allow(&mut self, origin: Origin) -> Result<(), OriginError> {
        if self.mode == TransportPolicyMode::ProductionRemote
            && (origin.scheme != Scheme::Https
                || origin.host == "localhost"
                || origin
                    .host
                    .parse::<IpAddr>()
                    .is_ok_and(|address| !is_public_address(address)))
        {
            return Err(OriginError::new(
                "production policy cannot admit a loopback or private origin",
            ));
        }
        if self.mode == TransportPolicyMode::TestLoopback && !is_loopback_host(&origin.host) {
            return Err(OriginError::new(
                "test-loopback policy can admit only loopback origins",
            ));
        }
        self.allowed.push(origin);
        Ok(())
    }

    /// Parses and adds one allowed origin URL.
    pub fn allow_url(&mut self, url: &str) -> Result<(), OriginError> {
        let origin = match self.mode {
            TransportPolicyMode::ProductionRemote => parse_production(url)?,
            TransportPolicyMode::TestLoopback => parse(url)?,
        };
        self.allow(origin)
    }

    /// Returns true when `url` is a strictly valid request that falls within a
    /// declared allowed origin (exact authority match plus path-prefix
    /// containment). Any parse failure denies. This form permits no query
    /// string and is used for registry index/download requests.
    pub fn permits(&self, url: &str) -> bool {
        self.authorizes_request(RequestKind::CargoRegistryIndex, url)
    }

    /// Returns true when `url` is a strictly valid outbound request of the given
    /// typed [`RequestKind`] that falls within a declared allowed origin.
    ///
    /// Query handling is typed, not a free-form escape hatch: registry index and
    /// crate-download requests permit no query string, while a Cargo Git smart
    /// transport request permits only the exact `/info/refs?service=git-upload-pack`
    /// semantics. Any fragment, any other query, and any parse failure deny.
    pub fn authorizes_request(&self, kind: RequestKind, url: &str) -> bool {
        let Ok(request) = self.parse_request_target(kind, url) else {
            return false;
        };
        self.allowed.iter().any(|allowed| {
            allowed.same_authority(&request) && path_within(&allowed.path, &request.path)
        })
    }

    fn parse_request_target(&self, kind: RequestKind, url: &str) -> Result<Origin, OriginError> {
        if url.contains('#') {
            return Err(OriginError::new("request URL must not contain a fragment"));
        }
        let base = match url.split_once('?') {
            None => url,
            Some((base, query)) => {
                match kind {
                    RequestKind::CargoGit => {
                        if query != "service=git-upload-pack" {
                            return Err(OriginError::new(
                                "a Cargo Git request may carry only `?service=git-upload-pack`",
                            ));
                        }
                        // The only query-bearing Git request is the smart-HTTP
                        // reference discovery at `<repo>/info/refs`.
                        if !base.ends_with("/info/refs") {
                            return Err(OriginError::new(
                                "the `service=git-upload-pack` query is permitted only on `/info/refs`",
                            ));
                        }
                    }
                    RequestKind::CargoRegistryIndex | RequestKind::CargoCrateDownload => {
                        return Err(OriginError::new(
                            "registry index and crate-download requests must carry no query string",
                        ));
                    }
                }
                base
            }
        };
        match self.mode {
            TransportPolicyMode::ProductionRemote => parse_production(base),
            TransportPolicyMode::TestLoopback => parse(base),
        }
    }

    /// Reauthorizes one outbound request URL after DNS resolution.
    ///
    /// A future transport MUST call this for the initial URL and for every
    /// redirect target through the same typed authorization API, with the
    /// addresses pinned to the resulting connection. This value type performs no
    /// resolution, socket operation, redirect following, or enforcement itself.
    pub fn authorizes_resolved(
        &self,
        kind: RequestKind,
        url: &str,
        addresses: &[IpAddr],
    ) -> bool {
        if !self.authorizes_request(kind, url) || addresses.is_empty() {
            return false;
        }
        match self.mode {
            TransportPolicyMode::ProductionRemote => {
                addresses.iter().copied().all(is_public_address)
            }
            TransportPolicyMode::TestLoopback => addresses.iter().copied().all(is_loopback_address),
        }
    }
}

fn is_loopback_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => address.is_loopback(),
        IpAddr::V6(address) => address.is_loopback(),
    }
}

fn is_public_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_public_ipv4(address),
        IpAddr::V6(address) => {
            // Classify an IPv4-mapped address (`::ffff:a.b.c.d`) by its embedded
            // IPv4 address so a mapped loopback or private address cannot pose
            // as a public IPv6 host.
            if let Some(mapped) = address.to_ipv4_mapped() {
                return is_public_ipv4(mapped);
            }
            let segments = address.segments();
            !(address.is_loopback()
                || address.is_unspecified()
                || address.is_multicast()
                || (segments[0] & 0xfe00) == 0xfc00
                || (segments[0] & 0xffc0) == 0xfe80)
        }
    }
}

fn is_public_ipv4(address: std::net::Ipv4Addr) -> bool {
    let octets = address.octets();
    !(address.is_private()
        || address.is_loopback()
        || address.is_link_local()
        || address.is_broadcast()
        || address.is_unspecified()
        || octets[0] == 0
        || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        || (octets[0] >= 224))
}

/// Returns true when `candidate` is at or below the `prefix` path, matching only
/// on whole path components so `/index` does not match `/index-secret`.
fn path_within(prefix: &str, candidate: &str) -> bool {
    let prefix = prefix.strip_suffix('/').unwrap_or(prefix);
    if prefix.is_empty() {
        return true;
    }
    if candidate == prefix {
        return true;
    }
    let mut with_separator = String::with_capacity(prefix.len() + 1);
    with_separator.push_str(prefix);
    with_separator.push('/');
    candidate.starts_with(&with_separator)
}

/// Confirms a value is exactly the canonical form of one origin URL: parsing it
/// and re-serializing yields an identical scheme, host, explicit-or-default
/// port, and path. This rejects non-canonical aliases (uppercase host, implied
/// vs. explicit default port mismatch, redundant separators) that could bypass
/// exact-origin comparisons.
pub fn is_canonical_origin_url(url: &str) -> bool {
    parse(url).is_ok_and(|origin| serialize(&origin) == url)
}

/// Returns true only for an exactly canonical production HTTPS origin.
pub fn is_canonical_production_origin_url(url: &str) -> bool {
    parse_production(url).is_ok_and(|origin| serialize(&origin) == url)
}

/// Serializes a parsed origin back to its canonical URL string.
fn serialize(origin: &Origin) -> String {
    let host = if origin.host.contains(':') && !origin.host.starts_with('[') {
        format!("[{}]", origin.host)
    } else {
        origin.host.clone()
    };
    if origin.port == origin.scheme.default_port() {
        format!("{}://{}{}", origin.scheme.as_str(), host, origin.path)
    } else {
        format!(
            "{}://{}:{}{}",
            origin.scheme.as_str(),
            host,
            origin.port,
            origin.path
        )
    }
}

/// Returns true when two origins share the same authority and one path is at or
/// below the other, i.e. they name overlapping request spaces. Two sources that
/// overlap this way are rejected because a request could satisfy both.
pub fn origins_overlap(left: &Origin, right: &Origin) -> bool {
    if !left.same_authority(right) {
        return false;
    }
    path_within(&left.path, &right.path) || path_within(&right.path, &left.path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_canonical_https_origin() {
        let origin = parse("https://index.crates.io/").expect("canonical origin");
        assert_eq!(origin.scheme(), Scheme::Https);
        assert_eq!(origin.host(), "index.crates.io");
        assert_eq!(origin.port(), 443);
        assert_eq!(origin.path(), "/");
    }

    #[test]
    fn lowercases_host_and_defaults_port() {
        let origin = parse("https://Index.Crates.IO/index/").expect("origin");
        assert_eq!(origin.host(), "index.crates.io");
        assert_eq!(origin.port(), 443);
        assert_eq!(origin.path(), "/index/");
    }

    #[test]
    fn rejects_userinfo_query_and_fragment() {
        assert!(parse("https://user:pass@host/").is_err());
        assert!(parse("https://host/index?token=1").is_err());
        assert!(parse("https://host/index#frag").is_err());
    }

    #[test]
    fn rejects_percent_encoding_and_traversal() {
        assert!(parse("https://host/a/%2e%2e/b").is_err());
        assert!(parse("https://host/a/../b").is_err());
        assert!(parse("https://host/a//b").is_err());
        assert!(parse("https://host/./a").is_err());
    }

    #[test]
    fn rejects_plain_http_for_remote_but_allows_loopback() {
        assert!(parse("http://index.crates.io/").is_err());
        assert!(parse("http://127.0.0.1:8080/index/").is_ok());
        assert!(parse("http://localhost/index/").is_ok());
        assert!(parse("http://[::1]:9000/index/").is_ok());
    }

    #[test]
    fn rejects_non_http_schemes_and_empty_host() {
        assert!(parse("ftp://host/").is_err());
        assert!(parse("file:///etc/passwd").is_err());
        assert!(parse("https:///index/").is_err());
    }

    #[test]
    fn canonical_origin_detection_rejects_aliases() {
        assert!(is_canonical_origin_url("https://index.crates.io/"));
        assert!(is_canonical_origin_url("http://127.0.0.1:8080/index/"));
        assert!(!is_canonical_origin_url("https://Index.Crates.io/"));
        assert!(!is_canonical_origin_url("https://index.crates.io:443/"));
        assert!(!is_canonical_origin_url("https://index.crates.io"));
    }

    #[test]
    fn policy_matches_exact_authority_and_path_prefix() {
        let mut policy = OriginPolicy::new();
        policy
            .allow_url("https://index.crates.io/index/")
            .expect("allow index");
        policy
            .allow_url("https://static.crates.io/crates/")
            .expect("allow download");

        assert!(policy.permits("https://index.crates.io/index/ab/cd/serde"));
        assert!(policy.permits("https://static.crates.io/crates/serde/serde-1.0.0.crate"));
    }

    #[test]
    fn policy_rejects_wrong_scheme_host_port_and_prefix() {
        let mut policy = OriginPolicy::new();
        policy
            .allow_url("https://index.crates.io/index/")
            .expect("allow index");

        // Wrong scheme.
        assert!(!policy.permits("http://index.crates.io/index/serde"));
        // Wrong host.
        assert!(!policy.permits("https://mirror.invalid/index/serde"));
        // Wrong port.
        assert!(!policy.permits("https://index.crates.io:8443/index/serde"));
        // Outside the declared path prefix.
        assert!(!policy.permits("https://index.crates.io/other/serde"));
        // Component-boundary evasion must not match.
        assert!(!policy.permits("https://index.crates.io/index-secret/serde"));
        // A redirect target to an undeclared origin is denied.
        assert!(!policy.permits("https://evil.invalid/index/serde"));
        // Credentials, query, and fragment in a request are denied.
        assert!(!policy.permits("https://user@index.crates.io/index/serde"));
        assert!(!policy.permits("https://index.crates.io/index/serde?x=1"));
    }

    #[test]
    fn production_policy_rejects_loopback_and_private_resolution() {
        let mut production = OriginPolicy::new();
        production
            .allow_url("https://registry.example.invalid/index/")
            .expect("public-name policy");
        assert!(!production.authorizes_resolved(
            RequestKind::CargoRegistryIndex,
            "https://registry.example.invalid/index/crate",
            &[IpAddr::from([127, 0, 0, 1])]
        ));
        assert!(!production.allow_url("http://127.0.0.1:8080/index/").is_ok());

        let mut fixture = OriginPolicy::test_loopback();
        fixture
            .allow_url("http://127.0.0.1:8080/index/")
            .expect("test loopback policy");
        assert!(fixture.authorizes_resolved(
            RequestKind::CargoRegistryIndex,
            "http://127.0.0.1:8080/index/crate",
            &[IpAddr::from([127, 0, 0, 1])]
        ));
    }

    #[test]
    fn production_parser_rejects_private_link_local_and_loopback_literals() {
        for origin in [
            "https://127.0.0.1/index/",
            "https://10.0.0.1/index/",
            "https://169.254.1.1/index/",
            "https://[::1]/index/",
            "https://[fe80::1]/index/",
        ] {
            assert!(
                parse_production(origin).is_err(),
                "production origin `{origin}` must be rejected"
            );
        }
    }

    #[test]
    fn origins_overlap_only_on_shared_authority_and_path_prefix() {
        let index = parse("https://index.crates.io/index/").expect("index");
        let nested = parse("https://index.crates.io/index/nested/").expect("nested");
        let sibling = parse("https://index.crates.io/other/").expect("sibling");
        let other_host = parse("https://static.crates.io/index/").expect("other host");
        assert!(origins_overlap(&index, &nested));
        assert!(origins_overlap(&nested, &index));
        assert!(!origins_overlap(&index, &sibling));
        assert!(!origins_overlap(&index, &other_host));
    }

    #[test]
    fn rejects_noncanonical_numeric_ipv4_aliases() {
        for alias in [
            "https://2130706433/index/",
            "https://127.1/index/",
            "https://127.0.1/index/",
            "https://0177.0.0.1/index/",
            "https://0x7f.0.0.1/index/",
            "https://0x7f000001/index/",
            "https://192.168.0.256/index/",
        ] {
            assert!(
                parse(alias).is_err(),
                "numeric IPv4 alias `{alias}` must be rejected"
            );
            assert!(
                parse_production(alias).is_err(),
                "numeric IPv4 alias `{alias}` must be rejected for production"
            );
        }
        // The single canonical dotted-decimal spelling still parses (and is then
        // rejected by production classification, not by the alias guard).
        assert!(parse("http://127.0.0.1/index/").is_ok());
        assert!(parse("https://93.184.216.34/index/").is_ok());
    }

    #[test]
    fn classifies_ipv4_mapped_ipv6_by_its_embedded_address() {
        for mapped in [
            "https://[::ffff:127.0.0.1]/index/",
            "https://[::ffff:10.0.0.1]/index/",
            "https://[::ffff:169.254.0.1]/index/",
        ] {
            assert!(
                parse_production(mapped).is_err(),
                "IPv4-mapped private/loopback `{mapped}` must be rejected"
            );
        }
        assert!(!is_public_address("::ffff:127.0.0.1".parse().expect("mapped")));
        assert!(is_public_address("::ffff:93.184.216.34".parse().expect("mapped")));
    }

    #[test]
    fn typed_query_parsing_is_scoped_to_cargo_git_info_refs() {
        let mut policy = OriginPolicy::new();
        policy
            .allow_url("https://git.example.invalid/dependency.git")
            .expect("allow git repository");

        // The exact smart-HTTP reference discovery is permitted for a Git kind.
        assert!(policy.authorizes_request(
            RequestKind::CargoGit,
            "https://git.example.invalid/dependency.git/info/refs?service=git-upload-pack"
        ));
        // No other query, path, service, or a fragment is permitted.
        assert!(!policy.authorizes_request(
            RequestKind::CargoGit,
            "https://git.example.invalid/dependency.git/info/refs?service=git-receive-pack"
        ));
        assert!(!policy.authorizes_request(
            RequestKind::CargoGit,
            "https://git.example.invalid/dependency.git/objects?service=git-upload-pack"
        ));
        assert!(!policy.authorizes_request(
            RequestKind::CargoGit,
            "https://git.example.invalid/dependency.git/info/refs?service=git-upload-pack#frag"
        ));
        // Registry kinds never accept a query string.
        let mut registry = OriginPolicy::new();
        registry
            .allow_url("https://index.crates.io/index/")
            .expect("allow index");
        assert!(!registry.authorizes_request(
            RequestKind::CargoRegistryIndex,
            "https://index.crates.io/index/serde?token=1"
        ));
        assert!(registry.authorizes_request(
            RequestKind::CargoRegistryIndex,
            "https://index.crates.io/index/serde"
        ));
    }

    #[test]
    fn redirect_targets_reuse_the_same_authorization_api() {
        let mut policy = OriginPolicy::new();
        policy
            .allow_url("https://static.crates.io/crates/")
            .expect("allow download");
        // A redirect to a declared origin with a public address is authorized;
        // a redirect to an undeclared origin, or one resolving to a private
        // address, is denied by the identical resolved-authorization check.
        assert!(policy.authorizes_resolved(
            RequestKind::CargoCrateDownload,
            "https://static.crates.io/crates/serde/serde-1.0.0.crate",
            &[IpAddr::from([93, 184, 216, 34])]
        ));
        assert!(!policy.authorizes_resolved(
            RequestKind::CargoCrateDownload,
            "https://evil.invalid/crates/serde/serde-1.0.0.crate",
            &[IpAddr::from([93, 184, 216, 34])]
        ));
        assert!(!policy.authorizes_resolved(
            RequestKind::CargoCrateDownload,
            "https://static.crates.io/crates/serde/serde-1.0.0.crate",
            &[IpAddr::from([127, 0, 0, 1])]
        ));
    }
}
