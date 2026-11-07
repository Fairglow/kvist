use crate::{
    Result, VendorLimits,
    integrity::{LockedGraph, Origin},
    unavailable,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Config {
    pub(crate) source: BTreeMap<String, Source>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub(crate) struct Source {
    #[serde(skip_serializing_if = "Option::is_none")]
    registry: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    git: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rev: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    replace_with: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    directory: Option<String>,
}

pub(crate) fn safe_url(raw: &str) -> bool {
    if raw.len() > 8192 || raw.chars().any(char::is_control) {
        return false;
    }
    let Ok(url) = url::Url::parse(raw.strip_prefix("sparse+").unwrap_or(raw)) else {
        return false;
    };
    matches!(url.scheme(), "https" | "http" | "ssh" | "git" | "file")
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
        && (url.scheme() == "file" || url.host_str().is_some())
}

fn name(raw: &str) -> bool {
    if let Some(url) = raw
        .strip_prefix("git+")
        .or_else(|| raw.strip_prefix("registry+"))
    {
        return safe_url(url);
    }
    if raw.contains("://") {
        return safe_url(raw);
    }
    !raw.is_empty()
        && raw.len() <= 255
        && raw
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
        && raw != "."
        && raw != ".."
}

pub(crate) fn destination(raw: &str) -> Result<()> {
    let path = Path::new(raw);
    if !path.is_absolute()
        || path == Path::new("/")
        || raw.contains("//")
        || raw.ends_with('/')
        || raw.len() > 32 * 1024
        || raw.chars().any(char::is_control)
    {
        return Err(unavailable(
            Path::new("resolver"),
            "resolver destination must be a bounded absolute directory",
        ));
    }
    crate::fs::absolute(path)?;
    Ok(())
}

pub(crate) fn parse(text: &str, limits: &VendorLimits) -> Result<Config> {
    let bad = || {
        unavailable(
            Path::new("resolver"),
            "invalid or unsupported source-replacement configuration",
        )
    };
    if text.len() as u64 > limits.max_metadata_bytes {
        return Err(bad());
    }
    let config: Config = toml::from_str(text).map_err(|_| bad())?;
    if config.source.is_empty() || config.source.len() > limits.max_sources {
        return Err(bad());
    }
    for (key, source) in &config.source {
        if !name(key) {
            return Err(bad());
        }
        let selectors = [&source.branch, &source.tag, &source.rev];
        if selectors
            .iter()
            .filter_map(|value| value.as_ref())
            .any(|value| {
                value.is_empty() || value.len() > 1024 || value.chars().any(char::is_control)
            })
        {
            return Err(bad());
        }
        if source.registry.as_ref().is_some_and(|url| !safe_url(url))
            || source.git.as_ref().is_some_and(|url| {
                !safe_url(url) || url::Url::parse(url).is_ok_and(|u| u.query().is_some())
            })
        {
            return Err(bad());
        }
        if let Some(dir) = &source.directory {
            if dir.is_empty()
                || dir.len() > 32 * 1024
                || dir.chars().any(char::is_control)
                || source.replace_with.is_some()
                || source.registry.is_some()
                || source.git.is_some()
                || selectors.iter().any(|x| x.is_some())
            {
                return Err(bad());
            }
        } else {
            let Some(target) = &source.replace_with else {
                return Err(bad());
            };
            if !name(target)
                || source.registry.is_some() && source.git.is_some()
                || selectors.iter().filter(|x| x.is_some()).count() > 1
                || source.git.is_none() && selectors.iter().any(|x| x.is_some())
                || (key != "crates-io" && source.registry.is_none() && source.git.is_none())
            {
                return Err(bad());
            }
            let Some(replacement) = config.source.get(target) else {
                return Err(bad());
            };
            // Cargo's generated replacement graph is one hop. Reject cycles/chains.
            if replacement.directory.is_none() || replacement.replace_with.is_some() {
                return Err(bad());
            }
        }
    }
    let referenced: BTreeSet<_> = config
        .source
        .values()
        .filter_map(|s| s.replace_with.as_ref())
        .collect();
    if config
        .source
        .iter()
        .any(|(key, source)| source.directory.is_some() && !referenced.contains(key))
    {
        return Err(bad());
    }
    Ok(config)
}

pub(crate) fn retarget(text: &str, target: &str, limits: &VendorLimits) -> Result<String> {
    destination(target)?;
    let mut config = parse(text, limits)?;
    for source in config.source.values_mut() {
        if source.directory.is_some() {
            source.directory = Some(target.into());
        }
    }
    toml::to_string(&config).map_err(|_| {
        unavailable(
            Path::new("resolver"),
            "cannot serialize source configuration",
        )
    })
}

pub(crate) fn bind(config: &Config, graph: &LockedGraph, published: bool) -> Result<()> {
    let bad = || {
        unavailable(
            &graph.project,
            "source replacement does not bind the locked source graph",
        )
    };
    if published
        && config
            .source
            .values()
            .filter_map(|s| s.directory.as_deref())
            .any(|dir| dir != crate::vendoring::VENDOR_SANDBOX_MOUNT)
    {
        return Err(bad());
    }
    for package in &graph.packages {
        let matches = match &package.origin {
            Origin::Local => continue,
            Origin::Registry { url, .. } => config
                .source
                .iter()
                .filter(|(key, source)| {
                    source.replace_with.is_some()
                        && match source.registry.as_deref() {
                            Some(registry) => registry == url,
                            None => {
                                source.git.is_none()
                                    && key.as_str() == "crates-io"
                                    && matches!(
                                        url.as_str(),
                                        "https://github.com/rust-lang/crates.io-index"
                                            | "sparse+https://index.crates.io/"
                                    )
                            }
                        }
                })
                .count(),
            Origin::Git { url, selector, .. } => config
                .source
                .values()
                .filter(|source| {
                    let actual = source
                        .branch
                        .as_ref()
                        .map(|s| ("branch", s))
                        .or_else(|| source.tag.as_ref().map(|s| ("tag", s)))
                        .or_else(|| source.rev.as_ref().map(|s| ("rev", s)));
                    source.replace_with.is_some()
                        && source.git.as_ref() == Some(url)
                        && actual.map(|(kind, value)| (kind, value.as_str()))
                            == selector
                                .as_ref()
                                .map(|(kind, value)| (kind.as_str(), value.as_str()))
                })
                .count(),
        };
        if matches != 1 {
            return Err(bad());
        }
    }
    Ok(())
}
