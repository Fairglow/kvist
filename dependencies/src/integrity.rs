use crate::{
    Error, Result,
    fs::{Dir, Identity, LOCK_LIMIT, METADATA_LIMIT, io_error, segment},
    unavailable,
    vendoring::{VendoringReport, lockfile_digest},
};
use sav::CancellationToken;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::{Path, PathBuf},
};

const MAX_ENTRIES: usize = 100_000;
const MAX_TOTAL: u64 = 1024 * 1024 * 1024;
const MAX_FILE: u64 = 256 * 1024 * 1024;
const MAX_PATHS: usize = 32 * 1024 * 1024;
const MAX_DEPTH: usize = 64;

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
        checksum: String,
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
    pub(crate) project: PathBuf,
    pub(crate) packages: Vec<Package>,
    pub(crate) digest: String,
    bytes: Vec<u8>,
    identity: Identity,
}

impl LockedGraph {
    pub(crate) fn read(project: &Dir, token: &CancellationToken) -> Result<Self> {
        control(token)?;
        let (bytes, identity) = project.read("Cargo.lock", LOCK_LIMIT)?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| unavailable(&project.display, "Cargo.lock is not UTF-8"))?;
        let lock: Lock = toml::from_str(text)
            .map_err(|_| unavailable(&project.display, "malformed or unsupported Cargo.lock"))?;
        if !matches!(lock.version, 3 | 4) || lock.package.len() > MAX_ENTRIES {
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
            let origin = match raw.source {
                None if raw.checksum.is_none() => Origin::Local,
                Some(source)
                    if source.starts_with("registry+") || source.starts_with("sparse+") =>
                {
                    let url = source.strip_prefix("registry+").unwrap_or(&source);
                    let Some(checksum) = raw.checksum else {
                        return Err(unavailable(&project.display, "registry checksum absent"));
                    };
                    if !crate::resolver::safe_url(url)
                        || !digest_shape(&checksum)
                        || url::Url::parse(url.strip_prefix("sparse+").unwrap_or(url))
                            .is_ok_and(|u| u.query().is_some())
                    {
                        return Err(unavailable(
                            &project.display,
                            "invalid registry source or checksum",
                        ));
                    }
                    Origin::Registry {
                        url: url.into(),
                        checksum,
                    }
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
            });
        }
        packages.sort_by(|a, b| (&a.name, &a.version).cmp(&(&b.name, &b.version)));
        // Cargo vendor cannot disambiguate identical name/version from distinct sources.
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
        Ok(Self {
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
        let (bytes, identity) = project.read("Cargo.lock", LOCK_LIMIT)?;
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
fn visit(
    directory: &Dir,
    prefix: &str,
    depth: usize,
    budget: &mut Budget,
    hashes: &mut BTreeMap<String, String>,
    token: &CancellationToken,
) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(unavailable(
            &directory.display,
            "vendor tree depth exceeded",
        ));
    }
    for name in directory.entries()? {
        control(token)?;
        let relative = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        budget.entries += 1;
        budget.paths += relative.len();
        if budget.entries > MAX_ENTRIES || budget.paths > MAX_PATHS {
            return Err(unavailable(
                &directory.display,
                "vendor entry or path budget exceeded",
            ));
        }
        let path = directory.path().join(&name);
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|e| io_error("inspect vendor entry", &directory.display.join(&name), e))?;
        if metadata.is_dir() {
            visit(
                &directory.child(&name)?,
                &relative,
                depth + 1,
                budget,
                hashes,
                token,
            )?;
        } else if metadata.is_file() {
            let mut file = directory.regular(&name, MAX_FILE)?;
            let before = Identity::of(&file, &directory.display.join(&name))?;
            let mut hash = Sha256::new();
            let mut size = 0u64;
            let mut buffer = [0; 64 * 1024];
            loop {
                control(token)?;
                let read = file
                    .read(&mut buffer)
                    .map_err(|e| io_error("hash vendor file", &directory.display.join(&name), e))?;
                if read == 0 {
                    break;
                }
                size += read as u64;
                budget.bytes += read as u64;
                if size > MAX_FILE || budget.bytes > MAX_TOTAL {
                    return Err(unavailable(
                        &directory.display,
                        "vendor byte budget exceeded",
                    ));
                }
                hash.update(&buffer[..read]);
            }
            if before != Identity::of(&file, &directory.display.join(&name))? {
                return Err(unavailable(
                    &directory.display,
                    "vendor file changed while hashing",
                ));
            }
            hashes.insert(relative, hex::encode(hash.finalize()));
        } else {
            return Err(unavailable(
                &directory.display,
                "vendor tree contains link or special file",
            ));
        }
    }
    Ok(())
}

pub(crate) fn validate_tree(
    directory: &Dir,
    token: &CancellationToken,
) -> Result<BTreeMap<String, String>> {
    let mut files = BTreeMap::new();
    visit(directory, "", 0, &mut Budget::default(), &mut files, token)?;
    Ok(files)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Checksum {
    package: serde_json::Value,
    #[serde(deserialize_with = "unique_files")]
    files: BTreeMap<String, String>,
    #[serde(rename = "$comment")]
    comment: Option<String>,
}

fn unique_files<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<BTreeMap<String, String>, D::Error> {
    struct Files;
    impl<'de> serde::de::Visitor<'de> for Files {
        type Value = BTreeMap<String, String>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("unique bounded checksum entries")
        }
        fn visit_map<M: serde::de::MapAccess<'de>>(
            self,
            mut access: M,
        ) -> std::result::Result<Self::Value, M::Error> {
            let mut files = BTreeMap::new();
            while let Some((key, value)) = access.next_entry::<String, String>()? {
                if files.len() >= MAX_ENTRIES || files.insert(key, value).is_some() {
                    return Err(serde::de::Error::custom(
                        "duplicate or excessive checksum entries",
                    ));
                }
            }
            Ok(files)
        }
    }
    deserializer.deserialize_map(Files)
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
    let all = validate_tree(vendor, token)?;
    let expected: BTreeMap<_, _> = graph
        .packages
        .iter()
        .filter(|package| !matches!(package.origin, Origin::Local))
        .map(|package| ((package.name.as_str(), package.version.as_str()), package))
        .collect();
    let mut found = BTreeSet::new();
    let mut observed = BTreeSet::new();
    for entry in vendor.entries()? {
        control(token)?;
        let directory = vendor.child(&entry)?;
        let (manifest, _) = directory.read("Cargo.toml", METADATA_LIMIT)?;
        let text = std::str::from_utf8(&manifest)
            .map_err(|_| unavailable(&directory.display, "package manifest is not UTF-8"))?;
        let manifest: CargoPackage = toml::from_str(text)
            .map_err(|_| unavailable(&directory.display, "invalid package manifest identity"))?;
        let name = manifest.package.name;
        let version = manifest.package.version;
        let Some(package) = expected.get(&(name.as_str(), version.as_str())).copied() else {
            return Err(unavailable(
                &directory.display,
                "vendor package is not locked",
            ));
        };
        if !found.insert((name, version)) {
            return Err(unavailable(&directory.display, "duplicate vendor identity"));
        }
        let (metadata, _) = directory.read(".cargo-checksum.json", METADATA_LIMIT)?;
        let checksum: Checksum = serde_json::from_slice(&metadata)
            .map_err(|_| unavailable(&directory.display, "corrupt Cargo checksum metadata"))?;
        if checksum
            .comment
            .as_ref()
            .is_some_and(|value| value.len() > 16 * 1024)
        {
            return Err(unavailable(
                &directory.display,
                "Cargo checksum comment exceeds bounds",
            ));
        }
        match &package.origin {
            Origin::Registry {
                checksum: expected, ..
            } if checksum.package.as_str() == Some(expected) => (),
            Origin::Git { .. } if checksum.package.is_null() => (),
            _ => {
                return Err(unavailable(
                    &directory.display,
                    "package checksum differs from locked source",
                ));
            }
        }
        observed.insert(format!("{entry}/.cargo-checksum.json"));
        for (file, expected) in checksum.files {
            if file.is_empty()
                || file.starts_with('/')
                || file.split('/').any(|p| !segment(p))
                || file == ".cargo-checksum.json"
                || !digest_shape(&expected)
            {
                return Err(unavailable(
                    &directory.display,
                    "unsafe checksum path or digest",
                ));
            }
            let path = format!("{entry}/{file}");
            if all.get(&path) != Some(&expected) {
                return Err(unavailable(
                    &directory.display,
                    "vendor content checksum mismatch",
                ));
            }
            observed.insert(path);
        }
    }
    if all.keys().any(|file| !observed.contains(file)) {
        return Err(unavailable(
            &vendor.display,
            "vendor contains undeclared files",
        ));
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
