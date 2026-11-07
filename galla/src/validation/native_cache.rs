//! Native data authority is separate from private Cargo administration.

use super::*;
use crate::protocol::{NativeRegistryAlias, NativeRegistryProtocol};

const REGISTRY_MOUNT: &str = "/workspace/cargo-registry";
const GIT_MOUNT: &str = "/workspace/cargo-git";

pub(super) fn validate_structure(
    request: &SandboxRequest,
    cache: &Cache,
    grants: &BTreeMap<&str, &Grant>,
) -> Result<(), ProtocolError> {
    let Some(native) = &cache.native else {
        return Ok(());
    };
    if request.phase == Phase::Authoring {
        return Err(invalid("native Cargo cache is not an authoring mode"));
    }
    validate_aliases(request, native.registries.as_deref().unwrap_or(&[]))?;
    if cache.writable.is_some() || cache.approved.is_some() || cache.promotion.is_some() {
        return Err(invalid(
            "native Cargo data mode forbids whole-home endpoints and promotion",
        ));
    }
    if cache.registry != REGISTRY_MOUNT || cache.git != GIT_MOUNT {
        return Err(invalid(
            "native Cargo registry/git must use their fixed data destinations",
        ));
    }
    let scratch = request
        .scratch
        .as_ref()
        .ok_or_else(|| invalid("native Cargo cache requires writable scratch"))?;
    if cache.cargo_home != child_path(&scratch.destination, "cargo-home")? {
        return Err(invalid(
            "native Cargo home must be exactly the private scratch/cargo-home directory",
        ));
    }
    let access = if request.phase == Phase::DependencyAcquisition {
        Access::ReadWrite
    } else {
        Access::ReadOnly
    };
    for (endpoint, destination, purpose, label) in [
        (
            &native.registry,
            REGISTRY_MOUNT,
            Purpose::Registry,
            "native registry",
        ),
        (
            &native.git,
            GIT_MOUNT,
            Purpose::DependencyCache,
            "native Git",
        ),
    ] {
        if let Some(endpoint) = endpoint {
            if endpoint.destination != destination {
                return Err(invalid(format!(
                    "{label} endpoint must be at {destination}"
                )));
            }
            validate_absolute(&endpoint.destination, label)?;
            validate_digest(&endpoint.identity, label)?;
            require_endpoint_grant(
                grants,
                destination,
                &endpoint.identity,
                access,
                purpose,
                label,
            )?;
        }
    }
    Ok(())
}

fn validate_aliases(
    request: &SandboxRequest,
    aliases: &[NativeRegistryAlias],
) -> Result<(), ProtocolError> {
    if aliases.len() > MAX_NETWORK_SOURCES {
        return Err(invalid(
            "native named registry aliases exceed the 64-entry limit",
        ));
    }
    let mut names = BTreeMap::new();
    for alias in aliases {
        if alias.name.len() > 64
            || !is_safe_registry_name(&alias.name)
            || alias.name.contains('.')
            || alias.name == CANONICAL_CRATES_IO_NAME
            || names
                .insert(alias.name.as_str(), alias.index_origin.as_str())
                .is_some()
        {
            return Err(invalid(
                "native registry aliases require unique 1-64 byte ASCII letters/digits/-/_ names other than crates-io",
            ));
        }
        bounded(&alias.index_origin, "native registry alias index origin")?;
        if (alias.protocol == NativeRegistryProtocol::Sparse && !alias.index_origin.ends_with('/'))
            || !origin::is_canonical_production_origin_url(&alias.index_origin)
        {
            return Err(invalid(
                "native registry alias index origin must be canonical production HTTPS without credentials, query, fragment or percent aliases; sparse origins must end in /",
            ));
        }
    }
    if request.phase == Phase::DependencyAcquisition {
        let approved: BTreeMap<_, _> = request
            .network
            .allowed_sources
            .iter()
            .filter_map(|source| match source {
                AllowedSource::CargoRegistry {
                    name, index_origin, ..
                } if name != CANONICAL_CRATES_IO_NAME => {
                    Some((name.as_str(), index_origin.as_str()))
                }
                _ => None,
            })
            .collect();
        if names != approved {
            return Err(invalid(
                "native acquisition registry aliases must exactly match additional allowed registry source names and index origins",
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_phase(
    request: &SandboxRequest,
    cache: &Cache,
    scratch: &Scratch,
) -> Result<(), ProtocolError> {
    let native = cache
        .native
        .as_ref()
        .ok_or_else(|| invalid("native Cargo mode requires its native field"))?;
    let acquisition = request.phase == Phase::DependencyAcquisition;
    let mut environment = vec![
        ("HOME", child_path(&scratch.destination, "home")?),
        ("PATH", RUNTIME_BIN_MOUNT.to_owned()),
        ("CARGO_HOME", cache.cargo_home.clone()),
        (
            "CARGO_TARGET_DIR",
            child_path(&scratch.destination, "target")?,
        ),
    ];
    let workspace_purpose = if acquisition {
        let lockfile = cache.lockfile.as_ref().ok_or_else(|| {
            invalid("native acquisition requires an exact writable lockfile workspace")
        })?;
        if request.working_directory != lockfile.destination {
            return Err(invalid(
                "native acquisition working_directory must equal its lockfile workspace",
            ));
        }
        environment.push(("CARGO_NET_GIT_FETCH_WITH_CLI", "false".to_owned()));
        Purpose::Lockfile
    } else {
        if cache.lockfile.is_some() {
            return Err(invalid(
                "native verification must not declare a writable lockfile",
            ));
        }
        let workspace_present = request.grants.iter().any(|grant| {
            grant.destination == request.working_directory
                && grant.purpose == Purpose::Verification
                && grant.access == Access::ReadOnly
        });
        if !workspace_present {
            return Err(invalid(
                "native verification working_directory requires its read-only workspace grant",
            ));
        }
        environment.push(("CARGO_NET_OFFLINE", "true".to_owned()));
        Purpose::Verification
    };
    if request.environment.get("PATH").map(String::as_str) != Some(RUNTIME_BIN_MOUNT) {
        return Err(invalid("native Cargo PATH must be exactly /workspace/bin"));
    }
    require_exact_environment(request, &request.environment, &environment)?;
    require_vendored_mount(request, Purpose::Runtime, RUNTIME_BIN_MOUNT)?;
    let mut purposes = vec![
        Purpose::Toolchain,
        Purpose::Runtime,
        Purpose::Scratch,
        workspace_purpose,
    ];
    if native.registry.is_some() {
        purposes.push(Purpose::Registry);
    }
    if native.git.is_some() {
        purposes.push(Purpose::DependencyCache);
    }
    ensure_exact_cargo_purposes(request, &purposes)
}
