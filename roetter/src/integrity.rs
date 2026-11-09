use crate::{
    Error, Result, VendorLimits,
    fs::{Dir, Identity, io_error, segment},
    unavailable,
    vendoring::{VendoringReport, lockfile_digest},
};
use sav::CancellationToken;
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

pub(crate) fn control(token: &CancellationToken) -> Result<()> {
    if token.is_cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}
pub(crate) fn digest_shape(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn package_name(text: &str) -> bool {
    segment(text)
        && text
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
        && text
            .bytes()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
}

#[derive(Debug, Clone)]
pub(crate) enum Origin {
    Local,
    Registry {
        url: String,
    },
    Git {
        url: String,
        selector: Option<(String, String)>,
    },
}
#[derive(Debug, Clone)]
pub(crate) struct Package {
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) origin: Origin,
    pub(crate) source: Option<String>,
    pub(crate) checksum: Option<String>,
}

#[derive(Deserialize)]
struct Lock {
    version: u32,
    #[serde(default)]
    package: Vec<RawPackage>,
}
#[derive(Deserialize)]
struct RawPackage {
    name: String,
    version: String,
    source: Option<String>,
    checksum: Option<String>,
}
pub(crate) struct LockedGraph {
    pub(crate) limits: VendorLimits,
    pub(crate) project: PathBuf,
    pub(crate) packages: Vec<Package>,
    pub(crate) digest: String,
    bytes: Vec<u8>,
    identity: Identity,
}

impl LockedGraph {
    pub(crate) fn read(project: &Dir, token: &CancellationToken) -> Result<Self> {
        Self::read_mode(project, token, false)
    }
    pub(crate) fn read_native(project: &Dir, token: &CancellationToken) -> Result<Self> {
        Self::read_mode(project, token, true)
    }
    fn read_mode(project: &Dir, token: &CancellationToken, native: bool) -> Result<Self> {
        control(token)?;
        let limits = VendorLimits::load(&project.display)?;
        let (bytes, identity) = project.read("Cargo.lock", limits.max_lockfile_bytes)?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| unavailable(&project.display, "Cargo.lock is not UTF-8"))?;
        let lock: Lock = toml::from_str(text)
            .map_err(|_| unavailable(&project.display, "malformed or unsupported Cargo.lock"))?;
        if !matches!(lock.version, 3 | 4) || lock.package.len() > limits.max_packages {
            return Err(unavailable(
                &project.display,
                "unsupported lock version or package bound",
            ));
        }
        let mut packages = Vec::new();
        let mut identities = BTreeSet::new();
        for raw in lock.package {
            control(token)?;
            if !package_name(&raw.name)
                || !segment(&raw.version)
                || semver::Version::parse(&raw.version).is_err()
            {
                return Err(unavailable(
                    &project.display,
                    "invalid locked package identity",
                ));
            }
            if !identities.insert((raw.name.clone(), raw.version.clone(), raw.source.clone())) {
                return Err(unavailable(
                    &project.display,
                    "duplicate locked package identity",
                ));
            }
            let origin = match raw.source.clone() {
                None if raw.checksum.is_none() => Origin::Local,
                Some(source)
                    if source.starts_with("registry+") || source.starts_with("sparse+") =>
                {
                    let url = source.strip_prefix("registry+").unwrap_or(&source);
                    let Some(checksum) = raw.checksum.as_deref() else {
                        return Err(unavailable(&project.display, "registry checksum absent"));
                    };
                    if !crate::resolver::safe_url(url)
                        || !digest_shape(checksum)
                        || url::Url::parse(url.strip_prefix("sparse+").unwrap_or(url))
                            .is_ok_and(|u| u.query().is_some())
                    {
                        return Err(unavailable(
                            &project.display,
                            "invalid registry source or checksum",
                        ));
                    }
                    Origin::Registry { url: url.into() }
                }
                Some(source) if source.starts_with("git+") && raw.checksum.is_none() => {
                    let raw_url = &source[4..];
                    let Some((address, revision)) = raw_url.rsplit_once('#') else {
                        return Err(unavailable(
                            &project.display,
                            "Git source has no immutable revision",
                        ));
                    };
                    if revision.len() != 40
                        || !revision
                            .bytes()
                            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                        || !crate::resolver::safe_url(address)
                    {
                        return Err(unavailable(
                            &project.display,
                            "invalid immutable Git source",
                        ));
                    }
                    let mut url = url::Url::parse(address)
                        .map_err(|_| unavailable(&project.display, "invalid Git URL"))?;
                    let params: Vec<_> = url
                        .query_pairs()
                        .map(|(k, v)| (k.into_owned(), v.into_owned()))
                        .collect();
                    if params.len() > 1
                        || params.iter().any(|(k, v)| {
                            !matches!(k.as_str(), "branch" | "tag" | "rev")
                                || v.is_empty()
                                || v.len() > 1024
                                || v.chars().any(char::is_control)
                        })
                    {
                        return Err(unavailable(&project.display, "invalid Git selector"));
                    }
                    url.set_query(None);
                    Origin::Git {
                        url: url.to_string(),
                        selector: params.into_iter().next(),
                    }
                }
                _ => return Err(unavailable(&project.display, "unsupported locked source")),
            };
            packages.push(Package {
                name: raw.name,
                version: raw.version,
                origin,
                source: raw.source,
                checksum: raw.checksum,
            });
        }
        packages.sort_by(|a, b| (&a.name, &a.version).cmp(&(&b.name, &b.version)));
        // Cargo vendor cannot disambiguate identical name/version from distinct sources.
        if !native {
            let mut external = BTreeSet::new();
            for package in &packages {
                if !matches!(package.origin, Origin::Local)
                    && !external.insert((&package.name, &package.version))
                {
                    return Err(unavailable(
                        &project.display,
                        "ambiguous external name/version sources are unsupported",
                    ));
                }
            }
        }
        Ok(Self {
            limits,
            project: project.display.clone(),
            digest: lockfile_digest(&bytes),
            bytes,
            identity,
            packages,
        })
    }
    pub(crate) fn external(&self) -> bool {
        self.packages
            .iter()
            .any(|p| !matches!(p.origin, Origin::Local))
    }
    pub(crate) fn unchanged(&self, project: &Dir, token: &CancellationToken) -> Result<()> {
        control(token)?;
        project.still_at_path()?;
        let (bytes, identity) = project.read("Cargo.lock", self.limits.max_lockfile_bytes)?;
        if bytes != self.bytes || identity != self.identity {
            return Err(Error::VendoringStale {
                path: self.project.to_string_lossy().into_owned(),
                current: lockfile_digest(&bytes),
                expected: self.digest.clone(),
            });
        }
        Ok(())
    }
    pub(crate) fn report(&self, vendor: &Path) -> VendoringReport {
        VendoringReport {
            project_root: self.project.to_string_lossy().into_owned(),
            lockfile_path: self
                .project
                .join("Cargo.lock")
                .to_string_lossy()
                .into_owned(),
            lockfile_digest: self.digest.clone(),
            vendored_dir: vendor.to_string_lossy().into_owned(),
            registry_dependencies: self
                .packages
                .iter()
                .filter(|p| matches!(p.origin, Origin::Registry { .. }))
                .count(),
            git_dependencies: self
                .packages
                .iter()
                .filter(|p| matches!(p.origin, Origin::Git { .. }))
                .count(),
            path_dependencies: self
                .packages
                .iter()
                .filter(|p| matches!(p.origin, Origin::Local))
                .count(),
            missing_dependencies: vec![],
        }
    }
}

#[derive(Default)]
struct Budget {
    entries: usize,
    bytes: u64,
    paths: usize,
}
pub(crate) fn validate_tree(
    directory: &Dir,
    limits: &VendorLimits,
    token: &CancellationToken,
    synchronize: bool,
) -> Result<()> {
    struct Frame {
        directory: Dir,
        prefix: String,
        entries: std::vec::IntoIter<String>,
    }
    limits.validate(&directory.display)?;
    let mut budget = Budget::default();
    let mut stack = vec![Frame {
        directory: Dir::open(&directory.display)?,
        prefix: String::new(),
        entries: directory.entries(limits.max_entries)?.into_iter(),
    }];
    while let Some(frame) = stack.last_mut() {
        control(token)?;
        let Some(name) = frame.entries.next() else {
            if synchronize {
                frame.directory.sync()?;
            }
            frame.directory.still_at_path()?;
            stack.pop();
            continue;
        };
        let relative = if frame.prefix.is_empty() {
            name.clone()
        } else {
            format!("{}/{name}", frame.prefix)
        };
        budget.entries = budget
            .entries
            .checked_add(1)
            .ok_or_else(|| unavailable(&directory.display, "vendor entry budget overflow"))?;
        budget.paths = budget
            .paths
            .checked_add(relative.len())
            .ok_or_else(|| unavailable(&directory.display, "vendor path budget overflow"))?;
        if budget.entries > limits.max_entries || budget.paths > limits.max_path_bytes {
            return Err(unavailable(
                &directory.display,
                "vendor entry or path budget exceeded",
            ));
        }
        let path = frame.directory.display.join(&name);
        let metadata = std::fs::symlink_metadata(frame.directory.path().join(&name))
            .map_err(|e| io_error("inspect vendor entry", &path, e))?;
        if metadata.is_dir() {
            let child = frame.directory.child(&name)?;
            if stack.len() > limits.max_depth {
                return Err(unavailable(&path, "vendor tree depth exceeded"));
            }
            let entries = child
                .entries(limits.max_entries - budget.entries)?
                .into_iter();
            stack.push(Frame {
                directory: child,
                prefix: relative,
                entries,
            });
        } else if metadata.is_file() {
            let file = frame.directory.regular(&name, limits.max_file_bytes)?;
            let before = Identity::of(&file, &path)?;
            let size = before.size;
            if size > limits.max_file_bytes {
                return Err(unavailable(&path, "vendor file byte budget exceeded"));
            }
            budget.bytes = budget
                .bytes
                .checked_add(size)
                .ok_or_else(|| unavailable(&path, "vendor byte budget overflow"))?;
            if budget.bytes > limits.max_total_bytes {
                return Err(unavailable(&path, "vendor byte budget exceeded"));
            }
            if synchronize {
                file.sync_all()
                    .map_err(|e| io_error("synchronize vendor file", &path, e))?;
            }
            if before != Identity::of(&file, &path)? {
                return Err(unavailable(&path, "vendor file changed during inspection"));
            }
        } else {
            return Err(unavailable(
                &path,
                "vendor tree contains link or special file",
            ));
        }
    }
    directory.still_at_path()
}
#[derive(Deserialize)]
struct CargoPackage {
    package: PackageIdentity,
}
#[derive(Deserialize)]
struct PackageIdentity {
    name: String,
    version: String,
}

pub(crate) fn catalogue(
    graph: &LockedGraph,
    vendor: &Dir,
    token: &CancellationToken,
) -> Result<VendoringReport> {
    let expected: BTreeSet<_> = graph
        .packages
        .iter()
        .filter(|package| !matches!(package.origin, Origin::Local))
        .map(|package| (package.name.as_str(), package.version.as_str()))
        .collect();
    let mut found = BTreeSet::new();
    for entry in vendor.entries(graph.limits.max_packages)? {
        control(token)?;
        let directory = vendor.child(&entry)?;
        let (manifest, _) = directory.read("Cargo.toml", graph.limits.max_metadata_bytes)?;
        let text = std::str::from_utf8(&manifest)
            .map_err(|_| unavailable(&directory.display, "package manifest is not UTF-8"))?;
        let manifest: CargoPackage = toml::from_str(text)
            .map_err(|_| unavailable(&directory.display, "invalid package manifest identity"))?;
        let name = manifest.package.name;
        let version = manifest.package.version;
        if !expected.contains(&(name.as_str(), version.as_str())) {
            return Err(unavailable(
                &directory.display,
                "vendor package is not locked",
            ));
        }
        if !found.insert((name, version)) {
            return Err(unavailable(&directory.display, "duplicate vendor identity"));
        }
        directory.regular(".cargo-checksum.json", graph.limits.max_metadata_bytes)?;
    }
    let mut report = graph.report(&vendor.display);
    report.missing_dependencies = graph
        .packages
        .iter()
        .filter(|p| {
            !matches!(p.origin, Origin::Local)
                && !found.contains(&(p.name.clone(), p.version.clone()))
        })
        .map(|p| format!("{}-{}", p.name, p.version))
        .collect();
    report.enforce()?;
    vendor.still_at_path()?;
    Ok(report)
}
