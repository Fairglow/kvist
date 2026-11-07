//! Scratch-local Cargo administration, never a copy of native data.

use std::io::{Read, Seek, Write};
use std::os::fd::{AsRawFd, OwnedFd};
use std::path::Path;

use nix::fcntl::{AtFlags, OFlag, openat};
use nix::sys::memfd::{MFdFlags, memfd_create};
use nix::sys::stat::{Mode, SFlag, fstat, fstatat};
use sha2::{Digest, Sha256};

use crate::protocol::{NativeRegistryAlias, NativeRegistryProtocol, Purpose, SandboxRequest};

pub(super) fn add(
    arguments: &mut Vec<String>,
    request: &SandboxRequest,
    sources: &[Option<OwnedFd>],
) -> Result<Option<OwnedFd>, String> {
    let Some(cache) = &request.cache else {
        return Ok(None);
    };
    let Some(native) = &cache.native else {
        return Ok(None);
    };
    for endpoint in [&native.registry, &native.git].into_iter().flatten() {
        let index = request
            .grants
            .iter()
            .position(|grant| grant.destination == endpoint.destination)
            .ok_or_else(|| "native Cargo data grant is missing".to_owned())?;
        let origin = sources
            .get(index)
            .and_then(Option::as_ref)
            .ok_or_else(|| "native Cargo data origin is missing".to_owned())?;
        let metadata =
            fstat(origin).map_err(|error| format!("cannot inspect native Cargo data: {error}"))?;
        if SFlag::from_bits_truncate(metadata.st_mode) & SFlag::S_IFMT != SFlag::S_IFDIR {
            return Err("native Cargo data origins must be ordinary directories".into());
        }
    }
    if let Some(lockfile) = &cache.lockfile {
        let index = request
            .grants
            .iter()
            .position(|grant| {
                grant.purpose == Purpose::Lockfile && grant.destination == lockfile.destination
            })
            .ok_or_else(|| "native Cargo lockfile workspace grant is missing".to_owned())?;
        let root = sources
            .get(index)
            .and_then(Option::as_ref)
            .ok_or_else(|| "native Cargo lockfile workspace origin is missing".to_owned())?;
        verify_lockfile(root, &lockfile.before_identity)?;
    }
    let scratch = request
        .scratch
        .as_ref()
        .ok_or_else(|| "native Cargo administrative state requires scratch".to_owned())?;
    let index = request
        .grants
        .iter()
        .position(|grant| {
            grant.purpose == Purpose::Scratch && grant.destination == scratch.destination
        })
        .ok_or_else(|| "native Cargo scratch grant is missing".to_owned())?;
    let root = sources
        .get(index)
        .and_then(Option::as_ref)
        .ok_or_else(|| "native Cargo scratch origin is missing".to_owned())?;
    let metadata =
        fstat(root).map_err(|error| format!("cannot inspect native Cargo scratch: {error}"))?;
    if SFlag::from_bits_truncate(metadata.st_mode) & SFlag::S_IFMT != SFlag::S_IFDIR {
        return Err("native Cargo scratch must be an ordinary directory".into());
    }
    let mut missing = Vec::new();
    // Check every destination before adding any backend operation. Existing
    // admin nodes are never reused: they could contain host configuration.
    for name in ["cargo-home", "home", "target"] {
        match fstatat(root, Path::new(name), AtFlags::AT_SYMLINK_NOFOLLOW) {
            Ok(metadata)
                if name != "cargo-home"
                    && SFlag::from_bits_truncate(metadata.st_mode) & SFlag::S_IFMT
                        == SFlag::S_IFDIR => {}
            Ok(_) => {
                return Err(format!(
                    "native Cargo scratch `{name}` conflicts with an existing caller path; select fresh scratch or explicitly remove that path, which the runner will not overwrite"
                ));
            }
            Err(nix::errno::Errno::ENOENT) => missing.push(name),
            Err(error) => {
                return Err(format!(
                    "cannot inspect native Cargo scratch `{name}`: {error}"
                ));
            }
        }
    }
    for name in missing {
        arguments.extend([
            "--perms".to_owned(),
            "0700".to_owned(),
            "--dir".to_owned(),
            format!("{}/{name}", scratch.destination),
        ]);
    }
    arguments.extend([
        "--perms".to_owned(),
        "0700".to_owned(),
        "--size".to_owned(),
        request.resources.max_scratch_bytes.to_string(),
        "--tmpfs".to_owned(),
        cache.cargo_home.clone(),
    ]);
    for (endpoint, name) in [(&native.registry, "registry"), (&native.git, "git")] {
        if let Some(endpoint) = endpoint {
            arguments.extend([
                "--symlink".to_owned(),
                endpoint.destination.clone(),
                format!("{}/{name}", cache.cargo_home),
            ]);
        } else {
            let destination = format!("{}/{name}", cache.cargo_home);
            arguments.extend([
                "--perms".to_owned(),
                "0555".to_owned(),
                "--size".to_owned(),
                request.resources.max_scratch_bytes.to_string(),
                "--tmpfs".to_owned(),
                destination.clone(),
                "--remount-ro".to_owned(),
                destination,
            ]);
        }
    }
    let aliases = native.registries.as_deref().unwrap_or(&[]);
    if aliases.is_empty() {
        return Ok(None);
    }
    let descriptor = registry_configuration(aliases)?;
    arguments.extend([
        "--perms".to_owned(),
        "0400".to_owned(),
        "--ro-bind-data".to_owned(),
        descriptor.as_raw_fd().to_string(),
        format!("{}/config.toml", cache.cargo_home),
    ]);
    Ok(Some(descriptor))
}

fn registry_configuration(aliases: &[NativeRegistryAlias]) -> Result<OwnedFd, String> {
    let mut aliases = aliases.iter().collect::<Vec<_>>();
    aliases.sort_by(|left, right| left.name.cmp(&right.name));
    let mut configuration = String::new();
    for alias in aliases {
        let name = serde_json::to_string(&alias.name)
            .map_err(|_| "cannot encode native registry alias name")?;
        let index = match alias.protocol {
            NativeRegistryProtocol::Git => serde_json::to_string(&alias.index_origin),
            NativeRegistryProtocol::Sparse => {
                serde_json::to_string(&format!("sparse+{}", alias.index_origin))
            }
        }
        .map_err(|_| "cannot encode native registry alias index")?;
        configuration.push_str(&format!("[registries.{name}]\nindex = {index}\n"));
    }
    let descriptor = memfd_create(c"galla-native-registries", MFdFlags::MFD_CLOEXEC)
        .map_err(|error| format!("cannot create private native registry configuration: {error}"))?;
    let mut file = std::fs::File::from(descriptor);
    file.write_all(configuration.as_bytes())
        .and_then(|()| file.rewind())
        .map_err(|error| {
            format!("cannot prepare private native registry configuration: {error}")
        })?;
    let descriptor: OwnedFd = file.into();
    nix::fcntl::fcntl(
        &descriptor,
        nix::fcntl::FcntlArg::F_SETFD(nix::fcntl::FdFlag::empty()),
    )
    .map_err(|error| format!("cannot hand native registry configuration to backend: {error}"))?;
    Ok(descriptor)
}

fn verify_lockfile(workspace: &OwnedFd, before: &str) -> Result<(), String> {
    let file = openat(
        workspace,
        Path::new("Cargo.lock"),
        OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC | OFlag::O_NONBLOCK,
        Mode::empty(),
    )
    .map_err(|error| format!("cannot open native Cargo.lock before identity: {error}"))?;
    let metadata =
        fstat(&file).map_err(|error| format!("cannot inspect native Cargo.lock: {error}"))?;
    if SFlag::from_bits_truncate(metadata.st_mode) & SFlag::S_IFMT != SFlag::S_IFREG {
        return Err("native Cargo.lock before identity requires an ordinary file".into());
    }
    let mut file = std::fs::File::from(file);
    let mut buffer = [0_u8; 16 * 1024];
    let mut hasher = Sha256::new();
    loop {
        let count = match file.read(&mut buffer) {
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => {
                return Err(format!(
                    "cannot read native Cargo.lock before identity: {error}"
                ));
            }
        };
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    if format!("sha256:{}", hex::encode(hasher.finalize())) != before {
        return Err(
            "native Cargo.lock before identity differs from the approved precondition; no payload ran, refresh the lockfile workspace request".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_configuration_preserves_git_index_and_default_sparse_protocol() {
        let aliases: Vec<NativeRegistryAlias> = serde_json::from_value(serde_json::json!([
            {"name":"git-alt","index_origin":"https://registry.example.invalid/git-index","protocol":"git"},
            {"name":"sparse-alt","index_origin":"https://registry.example.invalid/sparse/"}
        ])).unwrap();
        let mut configuration = String::new();
        std::fs::File::from(registry_configuration(&aliases).unwrap())
            .read_to_string(&mut configuration)
            .unwrap();
        assert_eq!(
            configuration,
            concat!(
                "[registries.\"git-alt\"]\nindex = \"https://registry.example.invalid/git-index\"\n",
                "[registries.\"sparse-alt\"]\nindex = \"sparse+https://registry.example.invalid/sparse/\"\n"
            )
        );
    }
}
