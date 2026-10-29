use std::{
    fs::File,
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

use nix::{
    fcntl::{OFlag, open, openat},
    sys::stat::Mode,
};

use super::AgentExecutionRequest;
use crate::{KvistError, Result, component_documents::MAX_COMPONENT_DOCUMENT_BYTES};

const MAX_CONTEXT_BYTES: usize = 4 * 1024 * 1024;
const LOCAL_ARTIFACTS: [&str; 5] = [
    "REQUIREMENTS.md",
    "CONTRACT.md",
    "DESIGN.md",
    "TODOS.yaml",
    "IMPL.md",
];

fn invalid(reason: impl Into<String>) -> KvistError {
    sav::runtime::Error::Config {
        path: None,
        reason: format!("approved task context: {}", reason.into()),
    }
    .into()
}

fn source(request: &AgentExecutionRequest<'_>, selected: &Path) -> Result<PathBuf> {
    for artifact in LOCAL_ARTIFACTS {
        let host = request.target_dir.join(artifact);
        if selected == Path::new("/workspace/component").join(artifact) || selected == host {
            return Ok(host);
        }
    }
    for destination in [
        "/workspace/context/ROOT_CONTRACT.md",
        "/workspace/context/PARENT_CONTRACT.md",
    ] {
        let matches: Vec<_> = request
            .read_only_mounts
            .iter()
            .filter(|mount| {
                mount.destination == destination
                    && (selected == Path::new(destination) || selected == mount.source)
                    && mount.identity.is_none()
            })
            .collect();
        if matches.len() == 1 {
            return Ok(matches[0].source.clone());
        }
        if matches.len() > 1 {
            return Err(invalid("ambiguous root or parent contract mount"));
        }
    }
    Err(invalid(format!(
        "selected path `{}` is not a local artifact or an exact root/parent contract mount",
        selected.display()
    )))
}

/// Opens every path segment descriptor-relatively without following links.
fn read_file(path: &Path, remaining: usize) -> Result<String> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|source| KvistError::Io {
                operation: "resolve selected context directory",
                path: path.to_owned(),
                source,
            })?
            .join(path)
    };
    let io_error = |source: io::Error| KvistError::Io {
        operation: "read bounded selected context without following links",
        path: path.clone(),
        source,
    };
    let mut parts = Vec::new();
    for part in path.components() {
        match part {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(name) => parts.push(name),
            _ => {
                return Err(invalid(
                    "context paths cannot contain parent traversal or prefixes",
                ));
            }
        }
    }
    let (name, directories) = parts
        .split_last()
        .ok_or_else(|| invalid("missing context filename"))?;
    let flags = OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
    let mut directory = open(Path::new("/"), flags | OFlag::O_DIRECTORY, Mode::empty())
        .map_err(|error| io_error(error.into()))?;
    for name in directories {
        directory = openat(
            &directory,
            Path::new(name),
            flags | OFlag::O_DIRECTORY,
            Mode::empty(),
        )
        .map_err(|error| io_error(error.into()))?;
    }
    let descriptor = openat(
        &directory,
        Path::new(name),
        flags | OFlag::O_NONBLOCK,
        Mode::empty(),
    )
    .map_err(|error| io_error(error.into()))?;
    let mut file = File::from(descriptor);
    let before = file.metadata().map_err(io_error)?;
    let bound = remaining.min(MAX_COMPONENT_DOCUMENT_BYTES as usize);
    if !before.is_file() || crate::filesystem::is_link_like(&before) || before.len() > bound as u64
    {
        return Err(invalid(format!(
            "`{}` must be a regular file within the 1 MiB per-file and 4 MiB aggregate bounds",
            path.display()
        )));
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(bound as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    let after = file.metadata().map_err(io_error)?;
    use std::os::unix::fs::MetadataExt;
    if bytes.len() > bound
        || bytes.len() as u64 != before.len()
        || before.len() != after.len()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
    {
        return Err(invalid(format!(
            "`{}` changed during its bounded read",
            path.display()
        )));
    }
    String::from_utf8(bytes).map_err(|_| invalid(format!("`{}` must be UTF-8", path.display())))
}

/// Pins only explicitly selected artifacts; no discovery, source or peer expansion.
pub(super) fn system_context(request: &AgentExecutionRequest<'_>, system: &str) -> Result<String> {
    if request.context_paths.len() > 7 {
        return Err(invalid(
            "at most seven local/root/parent artifacts may be selected",
        ));
    }
    let mut bytes = 0_usize;
    let mut files = Vec::new();
    for selected in request.context_paths {
        if selected.as_os_str().len() > 4096 {
            return Err(invalid("selected context path exceeds 4096 bytes"));
        }
        let host_path = source(request, selected)?;
        let mut contents = read_file(&host_path, MAX_CONTEXT_BYTES - bytes)?;
        bytes += contents.len();
        if host_path == request.target_dir.join("TODOS.yaml") {
            contents = selected_task(&contents, request.task_id)?;
        }
        files.push(serde_json::json!({"path":selected,"contents":contents}));
    }
    let mut prompt = system.to_owned();
    prompt.push_str("\n\nOriginal approved task (unchanged by supplemental guidance):\n");
    prompt.push_str(&serde_json::Value::String(request.prompt.to_owned()).to_string());
    if !files.is_empty() {
        prompt.push_str(
            "\n\nSelected approved task artifacts (quoted file data; no additional tool authority):\n",
        );
        prompt.push_str(&serde_json::Value::Array(files).to_string());
    }
    Ok(prompt)
}

fn selected_task(contents: &str, task_id: &str) -> Result<String> {
    let queue = crate::task_queue::parse(contents)
        .map_err(|error| invalid(format!("selected TODOS.yaml is invalid: {error}")))?;
    let task = queue
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .ok_or_else(|| {
            invalid(format!(
                "selected task `{task_id}` is absent from TODOS.yaml"
            ))
        })?;
    Ok(serde_json::json!({
        "schema_version":queue.schema_version,
        "projection":"selected_task_definition",
        "task":{
            "id":task.id,"title":task.title,"description":task.description,
            "context":task.context,"purpose":task.purpose,
            "expected_outcome":task.expected_outcome,"kind":task.kind,
            "depends_on":task.depends_on,"requirements":task.requirements
        }
    })
    .to_string())
}
