//! Read-only durable-artifact tracking inspection for supported VCSs.
//!
//! Git supplies exact index and ignore answers. Jujutsu does not expose a
//! read-only ignored-path query, so its inspection deliberately uses the
//! current saved working-copy snapshot and reports every absent path instead
//! of hiding a path that may be ignored.

use std::{
    collections::BTreeSet,
    ffi::OsString,
    io::Write,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
};

#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;

use crate::config::VcsSelection;

const MAX_VCS_ARGUMENT_BYTES: usize = 8 * 1024;

/// The stable result of a VCS tracking inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VcsInspection {
    /// Selected or detected VCS.
    pub vcs: Option<&'static str>,
    /// Root directory reported by the selected VCS.
    pub repository_root: Option<PathBuf>,
    /// Tracking status for every required durable artifact.
    pub artifacts: Vec<VcsArtifactStatus>,
    /// A concise status suitable for `kvist doctor`.
    pub summary: String,
    /// Detail for unavailable tools, absent repositories, or failed inspection.
    pub diagnostic: Option<String>,
}

/// The VCS status of one durable artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VcsArtifactStatus {
    /// Artifact path relative to the inspected Kvist project root.
    pub path: PathBuf,
    /// Tracking result under the selected VCS.
    pub state: VcsArtifactState,
}

/// A durable artifact's VCS tracking state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VcsArtifactState {
    /// The artifact is present and tracked by the selected VCS.
    Tracked,
    /// Git reports that the untracked artifact is ignored.
    Ignored,
    /// The artifact is present but untracked.
    Untracked,
    /// The artifact is required by a discovered component but does not exist.
    Missing,
    /// Jujutsu does not list the artifact in its saved working-copy snapshot.
    ///
    /// It can be untracked, ignored, excluded by `snapshot.auto-track`, or
    /// created after that snapshot. Kvist does not invoke a mutating Jujutsu
    /// command to distinguish those cases.
    NotTrackedByJujutsu,
    /// Kvist could not safely classify the artifact.
    Unknown,
}

impl VcsArtifactState {
    /// Stable text used by `kvist doctor`.
    pub const fn description(self) -> &'static str {
        match self {
            Self::Tracked => "tracked",
            Self::Ignored => "ignored",
            Self::Untracked => "untracked",
            Self::Missing => "missing from filesystem",
            Self::NotTrackedByJujutsu => "not tracked by jj snapshot (may be ignored or excluded)",
            Self::Unknown => "tracking status unavailable",
        }
    }
}

impl VcsInspection {
    /// Returns an inspection deliberately skipped because root state is not
    /// current and durable paths cannot be determined safely.
    pub fn not_checked(reason: impl Into<String>) -> Self {
        Self {
            vcs: None,
            repository_root: None,
            artifacts: Vec::new(),
            summary: "not checked".to_owned(),
            diagnostic: Some(reason.into()),
        }
    }

    fn unavailable(summary: impl Into<String>, diagnostic: impl Into<String>) -> Self {
        Self {
            vcs: None,
            repository_root: None,
            artifacts: Vec::new(),
            summary: summary.into(),
            diagnostic: Some(diagnostic.into()),
        }
    }

    fn complete(
        vcs: &'static str,
        repository_root: PathBuf,
        artifacts: Vec<VcsArtifactStatus>,
        diagnostic: Option<String>,
    ) -> Self {
        let complete = artifacts
            .iter()
            .all(|artifact| artifact.state == VcsArtifactState::Tracked);
        Self {
            vcs: Some(vcs),
            repository_root: Some(repository_root),
            artifacts,
            summary: if complete {
                "all required durable artifacts are tracked".to_owned()
            } else {
                "required durable artifacts need attention".to_owned()
            },
            diagnostic,
        }
    }
}

/// Inspects `required_paths` without staging, committing, or modifying either
/// VCS. Every path must be relative to `project_dir`.
pub fn inspect(
    project_dir: &Path,
    selection: VcsSelection,
    required_paths: impl IntoIterator<Item = PathBuf>,
) -> VcsInspection {
    let required_paths = required_paths.into_iter().collect::<BTreeSet<_>>();
    if let Some(path) = required_paths.iter().find(|path| !is_normal_relative(path)) {
        return VcsInspection::unavailable(
            "inspection failed",
            format!(
                "required durable artifact path `{}` must be a non-empty normal relative path",
                path.display()
            ),
        );
    }

    let git = detect_git(project_dir);
    let jj = detect_jj(project_dir);
    let selected = match select_vcs(selection, &git, &jj) {
        Ok(selected) => selected,
        Err(inspection) => return inspection,
    };

    match selected {
        DetectedVcs::Git { root } => inspect_git(project_dir, &root, &required_paths),
        DetectedVcs::Jujutsu { root } => inspect_jj(project_dir, &root, &required_paths),
    }
}

#[derive(Debug)]
enum DetectedVcs {
    Git { root: PathBuf },
    Jujutsu { root: PathBuf },
}

enum Detection {
    Found(PathBuf),
    Absent,
    ToolUnavailable,
    Failed(String),
}

fn select_vcs(
    selection: VcsSelection,
    git: &Detection,
    jj: &Detection,
) -> std::result::Result<DetectedVcs, VcsInspection> {
    match selection {
        VcsSelection::Git => select_explicit("Git", git),
        VcsSelection::Jujutsu => select_explicit("jj", jj),
        VcsSelection::Auto => match (git, jj) {
            (Detection::Found(_), Detection::Found(_)) => Err(VcsInspection::unavailable(
                "ambiguous VCS selection",
                "both Git and jj are present; set `vcs.kind` to `git` or `jj`",
            )),
            (Detection::Found(root), _) => Ok(DetectedVcs::Git { root: root.clone() }),
            (_, Detection::Found(root)) => Ok(DetectedVcs::Jujutsu { root: root.clone() }),
            (Detection::Failed(reason), _) | (_, Detection::Failed(reason)) => Err(
                VcsInspection::unavailable("inspection failed", reason.clone()),
            ),
            (Detection::ToolUnavailable, Detection::ToolUnavailable) => {
                Err(VcsInspection::unavailable(
                    "no supported VCS tool is available",
                    "install Git or jj, then run `kvist doctor` again",
                ))
            }
            _ => Err(VcsInspection::unavailable(
                "no supported VCS repository found",
                "initialize Git or jj, or select an available configured VCS",
            )),
        },
    }
}

fn select_explicit(
    name: &'static str,
    detection: &Detection,
) -> std::result::Result<DetectedVcs, VcsInspection> {
    match (name, detection) {
        ("Git", Detection::Found(root)) => Ok(DetectedVcs::Git { root: root.clone() }),
        ("jj", Detection::Found(root)) => Ok(DetectedVcs::Jujutsu { root: root.clone() }),
        (_, Detection::Absent) => Err(VcsInspection::unavailable(
            "configured VCS repository not found",
            format!(
                "`vcs.kind = \"{}\"` is configured, but no {name} repository contains this project",
                if name == "Git" { "git" } else { "jj" }
            ),
        )),
        (_, Detection::ToolUnavailable) => Err(VcsInspection::unavailable(
            "configured VCS tool is unavailable",
            format!("install {name} or select another `vcs.kind`"),
        )),
        (_, Detection::Failed(reason)) => Err(VcsInspection::unavailable(
            "inspection failed",
            reason.clone(),
        )),
        _ => unreachable!("only supported VCS names are passed to select_explicit"),
    }
}

fn detect_git(project_dir: &Path) -> Detection {
    match run("git", ["rev-parse", "--show-toplevel"], project_dir) {
        CommandResult::Success(output) => parse_repository_root("Git", output),
        CommandResult::Exit(reason) if is_not_repository("Git", &reason) => {
            if vcs_marker_exists(project_dir, ".git") {
                Detection::Failed(format!("cannot inspect Git repository: {reason}"))
            } else {
                Detection::Absent
            }
        }
        CommandResult::Exit(reason) => Detection::Failed(format!("cannot inspect Git: {reason}")),
        CommandResult::Unavailable => Detection::ToolUnavailable,
        CommandResult::Failed(reason) => Detection::Failed(format!("cannot inspect Git: {reason}")),
    }
}

fn detect_jj(project_dir: &Path) -> Detection {
    match run("jj", ["--ignore-working-copy", "root"], project_dir) {
        CommandResult::Success(output) => parse_repository_root("jj", output),
        CommandResult::Exit(reason) if is_not_repository("jj", &reason) => {
            if vcs_marker_exists(project_dir, ".jj") {
                Detection::Failed(format!("cannot inspect jj repository: {reason}"))
            } else {
                Detection::Absent
            }
        }
        CommandResult::Exit(reason) => Detection::Failed(format!("cannot inspect jj: {reason}")),
        CommandResult::Unavailable => Detection::ToolUnavailable,
        CommandResult::Failed(reason) => Detection::Failed(format!("cannot inspect jj: {reason}")),
    }
}

fn is_not_repository(vcs: &str, reason: &str) -> bool {
    match vcs {
        "Git" => reason.contains("not a git repository"),
        "jj" => reason.contains("There is no jj repo"),
        _ => false,
    }
}

fn vcs_marker_exists(project_dir: &Path, marker: &str) -> bool {
    project_dir
        .ancestors()
        .any(|directory| directory.join(marker).exists())
}

fn parse_repository_root(vcs: &str, output: Vec<u8>) -> Detection {
    let output = trim_command_line_ending(output);
    if output.is_empty() {
        return Detection::Failed(format!("{vcs} reported an empty repository root"));
    }
    let root = match path_from_bytes(&output, vcs) {
        Ok(root) => root,
        Err(diagnostic) => return Detection::Failed(diagnostic),
    };
    match root.canonicalize() {
        Ok(root) => Detection::Found(root),
        Err(error) => Detection::Failed(format!(
            "cannot resolve {vcs} repository root `{}`: {error}",
            root.display()
        )),
    }
}

fn inspect_git(
    project_dir: &Path,
    repository_root: &Path,
    required_paths: &BTreeSet<PathBuf>,
) -> VcsInspection {
    let project_relative = match project_relative_to_repository(project_dir, repository_root) {
        Ok(path) => path,
        Err(diagnostic) => return VcsInspection::unavailable("inspection failed", diagnostic),
    };
    let repository_paths = match required_paths
        .iter()
        .map(|path| git_repository_path(&project_relative.join(path)))
        .collect::<std::result::Result<Vec<_>, _>>()
    {
        Ok(paths) => paths,
        Err(diagnostic) => return VcsInspection::unavailable("inspection failed", diagnostic),
    };
    let (path_batches, unqueryable_paths) = batch_paths(repository_paths.clone());
    let mut tracked = BTreeSet::new();
    for batch in path_batches {
        let args = std::iter::once(OsString::from("ls-files"))
            .chain(std::iter::once(OsString::from("-z")))
            .chain(std::iter::once(OsString::from("--")))
            .chain(batch.iter().map(|path| path.clone().into_os_string()))
            .collect::<Vec<_>>();
        match run_owned("git", &args, repository_root) {
            CommandResult::Success(output) => match parse_paths(output, b'\0', "Git") {
                Ok(paths) => tracked.extend(paths),
                Err(diagnostic) => {
                    return VcsInspection::unavailable("inspection failed", diagnostic);
                }
            },
            CommandResult::Exit(reason) | CommandResult::Failed(reason) => {
                return VcsInspection::unavailable(
                    "inspection failed",
                    format!("cannot list Git-tracked durable artifacts: {reason}"),
                );
            }
            CommandResult::Unavailable => {
                return VcsInspection::unavailable(
                    "configured VCS tool is unavailable",
                    "Git was unavailable while inspecting tracking",
                );
            }
        }
    }

    let untracked_paths = repository_paths
        .iter()
        .filter(|path| !unqueryable_paths.contains(*path) && !tracked.contains(*path))
        .cloned()
        .collect::<Vec<_>>();
    let mut ignored = BTreeSet::new();
    for batch in batch_paths(untracked_paths).0 {
        match git_ignored(repository_root, &batch) {
            Ok(paths) => ignored.extend(paths),
            Err(diagnostic) => {
                return VcsInspection::unavailable(
                    "inspection failed",
                    format!("cannot inspect Git ignore rules: {diagnostic}"),
                );
            }
        }
    }

    let mut artifacts = Vec::with_capacity(required_paths.len());
    for (required_path, repository_path) in required_paths.iter().zip(repository_paths) {
        let state = if !project_dir.join(required_path).is_file() {
            VcsArtifactState::Missing
        } else if unqueryable_paths.contains(&repository_path) {
            VcsArtifactState::Unknown
        } else if tracked.contains(&repository_path) {
            VcsArtifactState::Tracked
        } else if ignored.contains(&repository_path) {
            VcsArtifactState::Ignored
        } else {
            VcsArtifactState::Untracked
        };
        artifacts.push(VcsArtifactStatus {
            path: required_path.clone(),
            state,
        });
    }

    VcsInspection::complete("Git", repository_root.to_path_buf(), artifacts, None)
}

fn git_ignored(
    repository_root: &Path,
    repository_paths: &[PathBuf],
) -> std::result::Result<BTreeSet<PathBuf>, String> {
    let args = [
        OsString::from("check-ignore"),
        OsString::from("--no-index"),
        OsString::from("-z"),
        OsString::from("--stdin"),
    ];
    let input = repository_paths
        .iter()
        .flat_map(|path| {
            let mut bytes = path.as_os_str().as_encoded_bytes().to_vec();
            bytes.push(b'\0');
            bytes
        })
        .collect::<Vec<_>>();
    let output = run_with_input("git", &args, repository_root, &input)?;
    parse_paths(output, b'\0', "Git")
}

fn git_repository_path(path: &Path) -> std::result::Result<PathBuf, String> {
    let mut git_path = OsString::new();
    for component in path.components() {
        let Component::Normal(segment) = component else {
            return Err(format!(
                "Git repository path `{}` must contain only normal components",
                path.display()
            ));
        };
        if !git_path.is_empty() {
            git_path.push("/");
        }
        git_path.push(segment);
    }
    if git_path.is_empty() {
        return Err("Git repository path must not be empty".to_owned());
    }
    Ok(PathBuf::from(git_path))
}

fn inspect_jj(
    project_dir: &Path,
    repository_root: &Path,
    required_paths: &BTreeSet<PathBuf>,
) -> VcsInspection {
    let project_relative = match project_relative_to_repository(project_dir, repository_root) {
        Ok(path) => path,
        Err(diagnostic) => return VcsInspection::unavailable("inspection failed", diagnostic),
    };
    let mut uninspectable_paths = BTreeSet::new();
    let mut queries = Vec::new();
    for path in required_paths {
        let repository_path = project_relative.join(path);
        match jj_fileset(&repository_path) {
            Ok(fileset) if fileset.len() <= MAX_VCS_ARGUMENT_BYTES => {
                queries.push(JjQuery { fileset });
            }
            Err(()) => {
                uninspectable_paths.insert(repository_path);
            }
            Ok(_) => {
                uninspectable_paths.insert(repository_path);
            }
        }
    }
    let mut tracked = BTreeSet::new();
    for batch in batch_jj_queries(queries) {
        let args = [
            OsString::from("--ignore-working-copy"),
            OsString::from("--color"),
            OsString::from("never"),
            OsString::from("--no-pager"),
            OsString::from("--quiet"),
            OsString::from("file"),
            OsString::from("list"),
            OsString::from("-r"),
            OsString::from("@"),
            OsString::from("-T"),
            OsString::from(r#"path ++ "\0""#),
            OsString::from("--"),
        ]
        .into_iter()
        .chain(batch.iter().map(|query| OsString::from(&query.fileset)))
        .collect::<Vec<_>>();
        match run_owned("jj", &args, repository_root) {
            CommandResult::Success(output) => match parse_paths(output, b'\0', "jj") {
                Ok(paths) => tracked.extend(paths),
                Err(diagnostic) => {
                    return VcsInspection::unavailable("inspection failed", diagnostic);
                }
            },
            CommandResult::Exit(reason) | CommandResult::Failed(reason) => {
                return VcsInspection::unavailable(
                    "inspection failed",
                    format!("cannot list jj tracked durable artifacts: {reason}"),
                );
            }
            CommandResult::Unavailable => {
                return VcsInspection::unavailable(
                    "configured VCS tool is unavailable",
                    "jj was unavailable while inspecting tracking",
                );
            }
        }
    }

    let artifacts = required_paths
        .iter()
        .map(|required_path| {
            let repository_path = project_relative.join(required_path);
            let state = if !project_dir.join(required_path).is_file() {
                VcsArtifactState::Missing
            } else if uninspectable_paths.contains(&repository_path) {
                VcsArtifactState::Unknown
            } else if tracked.contains(&repository_path) {
                VcsArtifactState::Tracked
            } else {
                VcsArtifactState::NotTrackedByJujutsu
            };
            VcsArtifactStatus {
                path: required_path.clone(),
                state,
            }
        })
        .collect();

    VcsInspection::complete(
        "jj",
        repository_root.to_path_buf(),
        artifacts,
        Some(
            "jj inspection uses its saved working-copy snapshot without snapshotting; a non-listed artifact may be ignored, excluded by `snapshot.auto-track`, or newer than that snapshot"
                .to_owned(),
        ),
    )
}

fn jj_fileset(path: &Path) -> std::result::Result<String, ()> {
    let path = path
        .components()
        .map(|component| match component {
            Component::Normal(segment) => segment.to_str().ok_or(()),
            _ => Err(()),
        })
        .collect::<std::result::Result<Vec<_>, _>>()?
        .join("/");
    Ok(format!(
        "root:\"{}\"",
        path.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

struct JjQuery {
    fileset: String,
}

fn batch_paths(paths: Vec<PathBuf>) -> (Vec<Vec<PathBuf>>, BTreeSet<PathBuf>) {
    let mut batches = Vec::new();
    let mut unqueryable = BTreeSet::new();
    let mut batch = Vec::new();
    let mut batch_bytes = 0;
    for path in paths {
        let path_bytes = path.as_os_str().as_encoded_bytes().len() + 1;
        if path_bytes > MAX_VCS_ARGUMENT_BYTES {
            unqueryable.insert(path);
            continue;
        }
        if batch_bytes + path_bytes > MAX_VCS_ARGUMENT_BYTES && !batch.is_empty() {
            batches.push(batch);
            batch = Vec::new();
            batch_bytes = 0;
        }
        batch_bytes += path_bytes;
        batch.push(path);
    }
    if !batch.is_empty() {
        batches.push(batch);
    }
    (batches, unqueryable)
}

fn batch_jj_queries(queries: Vec<JjQuery>) -> Vec<Vec<JjQuery>> {
    let mut batches = Vec::new();
    let mut batch = Vec::new();
    let mut batch_bytes = 0;
    for query in queries {
        let query_bytes = query.fileset.len() + 1;
        if batch_bytes + query_bytes > MAX_VCS_ARGUMENT_BYTES && !batch.is_empty() {
            batches.push(batch);
            batch = Vec::new();
            batch_bytes = 0;
        }
        batch_bytes += query_bytes;
        batch.push(query);
    }
    if !batch.is_empty() {
        batches.push(batch);
    }
    batches
}

fn project_relative_to_repository(
    project_dir: &Path,
    repository_root: &Path,
) -> std::result::Result<PathBuf, String> {
    let project_dir = project_dir.canonicalize().map_err(|error| {
        format!(
            "cannot resolve project directory `{}` for VCS inspection: {error}",
            project_dir.display()
        )
    })?;
    project_dir
        .strip_prefix(repository_root)
        .map(Path::to_path_buf)
        .map_err(|_| {
            format!(
                "project directory `{}` is outside selected repository `{}`",
                project_dir.display(),
                repository_root.display()
            )
        })
}

fn parse_paths(
    output: Vec<u8>,
    separator: u8,
    vcs: &str,
) -> std::result::Result<BTreeSet<PathBuf>, String> {
    output
        .split(|byte| *byte == separator)
        .filter(|path| !path.is_empty())
        .map(|path| path_from_bytes(path, vcs))
        .collect()
}

fn trim_command_line_ending(mut output: Vec<u8>) -> Vec<u8> {
    if output.last() == Some(&b'\n') {
        output.pop();
    }
    #[cfg(windows)]
    if output.last() == Some(&b'\r') {
        output.pop();
    }
    output
}

fn path_from_bytes(bytes: &[u8], vcs: &str) -> std::result::Result<PathBuf, String> {
    #[cfg(unix)]
    {
        let _ = vcs;
        Ok(PathBuf::from(OsString::from_vec(bytes.to_vec())))
    }
    #[cfg(not(unix))]
    {
        String::from_utf8(bytes.to_vec())
            .map(PathBuf::from)
            .map_err(|_| format!("{vcs} returned a non-UTF-8 path"))
    }
}

fn is_normal_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

enum CommandResult {
    Success(Vec<u8>),
    Exit(String),
    Unavailable,
    Failed(String),
}

fn run<const N: usize>(program: &str, args: [&str; N], directory: &Path) -> CommandResult {
    let args = args.into_iter().map(OsString::from).collect::<Vec<_>>();
    run_owned(program, &args, directory)
}

fn run_owned(program: &str, args: &[OsString], directory: &Path) -> CommandResult {
    let output = Command::new(program)
        .args(args)
        .current_dir(directory)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .output();
    match output {
        Ok(output) if output.status.success() => CommandResult::Success(output.stdout),
        Ok(output) => {
            CommandResult::Exit(format_process_failure(&output.stderr, output.status.code()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => CommandResult::Unavailable,
        Err(error) => CommandResult::Failed(error.to_string()),
    }
}

fn run_with_input(
    program: &str,
    args: &[OsString],
    directory: &Path,
    input: &[u8],
) -> std::result::Result<Vec<u8>, String> {
    let mut child = Command::new(program)
        .args(args)
        .current_dir(directory)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot start Git: {error}"))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "cannot open Git standard input".to_owned())?;
    stdin
        .write_all(input)
        .map_err(|error| format!("cannot write Git ignore query: {error}"))?;
    drop(stdin);
    let output = child
        .wait_with_output()
        .map_err(|error| format!("cannot read Git ignore result: {error}"))?;
    if output.status.success() || output.status.code() == Some(1) {
        Ok(output.stdout)
    } else {
        Err(format_process_failure(&output.stderr, output.status.code()))
    }
}

fn format_process_failure(stderr: &[u8], code: Option<i32>) -> String {
    let stderr = String::from_utf8_lossy(stderr);
    let detail = stderr.trim();
    match (code, detail.is_empty()) {
        (Some(code), true) => format!("exit status {code}"),
        (Some(code), false) => format!("exit status {code}: {detail}"),
        (None, true) => "terminated without an exit status".to_owned(),
        (None, false) => format!("terminated without an exit status: {detail}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_relative_paths_reject_escape_and_root_forms() {
        assert!(is_normal_relative(Path::new("src/SPEC.md")));
        for path in ["", ".", "../SPEC.md", "/SPEC.md"] {
            assert!(!is_normal_relative(Path::new(path)), "{path}");
        }
    }

    #[test]
    fn auto_selection_rejects_colocated_repositories() {
        let inspection = select_vcs(
            VcsSelection::Auto,
            &Detection::Found(PathBuf::from("/git")),
            &Detection::Found(PathBuf::from("/jj")),
        )
        .expect_err("colocated VCSs need an explicit selection");

        assert_eq!(inspection.summary, "ambiguous VCS selection");
    }

    #[test]
    fn jj_filesets_use_forward_slash_component_separators() {
        assert_eq!(
            jj_fileset(&Path::new("src").join("SPEC.md")).expect("UTF-8 path"),
            r#"root:"src/SPEC.md""#
        );
    }

    #[test]
    fn git_repository_paths_use_forward_slash_component_separators() {
        let path = git_repository_path(&Path::new("src").join("DOCS.md")).expect("normal Git path");

        assert_eq!(path.as_os_str().as_encoded_bytes(), b"src/DOCS.md");
    }

    #[test]
    fn path_batches_bound_arguments_and_isolate_unqueryable_paths() {
        let short_path = PathBuf::from("a".repeat(MAX_VCS_ARGUMENT_BYTES / 2));
        let oversized_path = PathBuf::from("b".repeat(MAX_VCS_ARGUMENT_BYTES + 1));
        let (batches, unqueryable) = batch_paths(vec![
            short_path.clone(),
            short_path.clone(),
            oversized_path.clone(),
        ]);

        assert_eq!(batches.len(), 2);
        assert!(batches.iter().all(|batch| {
            batch
                .iter()
                .map(|path| path.as_os_str().as_encoded_bytes().len() + 1)
                .sum::<usize>()
                <= MAX_VCS_ARGUMENT_BYTES
        }));
        assert_eq!(unqueryable, BTreeSet::from([oversized_path]));
    }
}
