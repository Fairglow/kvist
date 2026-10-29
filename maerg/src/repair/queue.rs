//! Recovery candidates never bypass the execution queue validator.

use std::collections::{BTreeMap, BTreeSet};

use serde_yaml::Value;

use crate::task_queue::{
    self, ComponentState, Revalidation, RevalidationState, StalenessCause, StalenessCauseKind,
    Task, TaskQueue, TaskStatus, TaskTimestamps, Timestamp,
};

pub(super) struct Candidate {
    pub contents: String,
    pub reasons: Vec<String>,
}

pub(super) fn unaccepted_component() -> ComponentState {
    let zero = format!("sha256:{}", "0".repeat(64));
    ComponentState {
        requirements_revision: zero.clone(),
        contract_revision: zero.clone(),
        design_revision: zero.clone(),
        parent_contract: None,
        revalidation: Revalidation {
            state: RevalidationState::Stale,
            checked_at: Timestamp::default_instant(),
            stale_since: Some(Timestamp::default_instant()),
            causes: vec![StalenessCause {
                kind: StalenessCauseKind::ComponentRequirementsRevisionChanged,
                path: "recovery:unaccepted-draft (synthetic sentinel, not observed intent)"
                    .to_owned(),
                expected_revision: zero,
                observed_revision: format!("sha256:{}", "1".repeat(64)),
            }],
        },
    }
}

pub(super) fn empty_queue() -> std::result::Result<String, String> {
    task_queue::serialize(&TaskQueue {
        schema_version: 1,
        component: unaccepted_component(),
        tasks: Vec::new(),
    })
    .map_err(|error| error.to_string())
}

pub(super) fn candidate(contents: &str, salvage: bool) -> std::result::Result<Candidate, String> {
    let mut reasons = Vec::new();
    let value: Value = match serde_yaml::from_str(contents) {
        Ok(value) => value,
        Err(error) if salvage => recover_fragments(contents, &mut reasons)
            .map_err(|reason| format!("{error}; fragment recovery unavailable: {reason}"))?,
        Err(error) => return Err(error.to_string()),
    };
    if value["schema_version"].as_u64() != Some(1) {
        return Err(
            "unsupported or missing schema; use an approved --reset, not migration".to_owned(),
        );
    }
    let mut queue = if salvage {
        salvage_queue(value, &mut reasons)?
    } else {
        serde_yaml::from_str::<TaskQueue>(contents).map_err(|error| error.to_string())?
    };
    if salvage {
        queue.component.revalidation = unaccepted_component().revalidation;
        reasons.push("invalidate prior revalidation authority: recovered queue is explicitly stale and requires component acceptance before task selection".to_owned());
    }
    if queue
        .tasks
        .iter()
        .any(|task| task.recovery_state.is_some() || task.status == TaskStatus::InProgress)
    {
        return Err("active/fenced tasks require `kvist task recover` when evidence permits, explicit disposition otherwise, or an approved --clean-slate; execution effects remain unknown".to_owned());
    }
    task_queue::normalize_set_lists(&mut queue);
    let (ordered, remaining) = ordered_tasks(queue.tasks);
    if !remaining.is_empty() && !salvage {
        return Err("duplicate IDs, missing dependencies or a dependency cycle require --salvage or --reset".to_owned());
    }
    for task in remaining {
        reasons.push(format!(
            "remove task `{}`: missing dependency, dependency cycle, or dependant of an unrecoverable task; no dependency edges are weakened",
            task.id
        ));
    }
    queue.tasks = Vec::new();
    for task in ordered {
        let id = task.id.clone();
        queue.tasks.push(task);
        if let Err(error) = task_queue::validate(&queue) {
            if !salvage {
                return Err(error.to_string());
            }
            queue.tasks.pop();
            reasons.push(format!("remove task `{id}`: {error}"));
        }
    }
    // Removing a lifecycle-invalid task also removes its dependent closure.
    // Validation of each prefix above rejects dangling edges without dropping them.
    let contents = task_queue::serialize(&queue).map_err(|error| error.to_string())?;
    reasons.push("normalize set lists and stable dependency declaration order; status/data changes and task removals are listed above; retained dependency edges are not weakened".to_owned());
    Ok(Candidate { contents, reasons })
}

fn recover_fragments(
    contents: &str,
    reasons: &mut Vec<String>,
) -> std::result::Result<Value, String> {
    for line in contents.lines() {
        if let Some((field, value)) = line.trim().split_once(':') {
            if field == "recovery_state" && value.trim() != "null" {
                return Err(
                    "unreadable queue may contain fenced execution; use approved --clean-slate"
                        .to_owned(),
                );
            }
            if field == "status"
                && serde_yaml::from_str::<Value>(value)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .as_deref()
                    == Some("in-progress")
            {
                return Err(
                    "unreadable queue may contain active execution; use approved --clean-slate"
                        .to_owned(),
                );
            }
        }
    }
    let lines = contents.lines().collect::<Vec<_>>();
    let boundary = lines
        .iter()
        .position(|line| {
            line.strip_prefix("tasks:")
                .is_some_and(|suffix| suffix.trim().is_empty() || suffix.trim().starts_with('#'))
        })
        .ok_or("no standalone top-level tasks header; use --reset")?;
    let mut header: Value = serde_yaml::from_str(&lines[..boundary].join("\n"))
        .map_err(|error| format!("unreadable queue header: {error}; use --reset"))?;
    if header["schema_version"].as_u64() != Some(1) {
        return Err("unsupported/missing schema; reset explicitly instead of migrating".to_owned());
    }
    let body = &lines[boundary + 1..];
    let indent = body
        .iter()
        .filter_map(|line| {
            let trimmed = line.trim_start_matches(' ');
            trimmed
                .starts_with("- ")
                .then_some(line.len() - trimmed.len())
        })
        .min()
        .ok_or("no independently parseable task entries; use --reset")?;
    let starts = body
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            (line
                .as_bytes()
                .get(..indent)
                .is_some_and(|prefix| prefix.iter().all(|byte| *byte == b' '))
                && line
                    .get(indent..)
                    .is_some_and(|suffix| suffix.starts_with("- ")))
            .then_some(index)
        })
        .collect::<Vec<_>>();
    let mut tasks = Vec::new();
    for (number, start) in starts.iter().copied().enumerate() {
        let end = starts.get(number + 1).copied().unwrap_or(body.len());
        let fragment = body[start..end].join("\n");
        match serde_yaml::from_str::<Vec<Value>>(&fragment) {
            Ok(mut entries) if entries.len() == 1 => tasks.append(&mut entries),
            Ok(_) => reasons.push(format!("quarantine ambiguous task fragment {number}; original bytes remain in backup")),
            Err(error) => reasons.push(format!("quarantine unreadable task fragment {number}: {error}; original bytes remain in backup")),
        }
    }
    reasons.push("salvage independently parseable same-indent task fragments from malformed YAML; segmentation is an explicit assumption, not reconstruction of original meaning".to_owned());
    header
        .as_mapping_mut()
        .ok_or("queue header must be a mapping")?
        .insert(Value::String("tasks".to_owned()), Value::Sequence(tasks));
    Ok(header)
}

fn salvage_queue(
    value: Value,
    reasons: &mut Vec<String>,
) -> std::result::Result<TaskQueue, String> {
    let mapping = value.as_mapping().ok_or("queue must be a mapping")?;
    for key in mapping.keys() {
        if !matches!(key.as_str(), Some("schema_version" | "component" | "tasks")) {
            reasons.push(format!(
                "discard unknown top-level field {key:?}; original retained in backup"
            ));
        }
    }
    let component = serde_yaml::from_value::<ComponentState>(value["component"].clone()).and_then(
        |component| {
            task_queue::validate_component(&component)
                .map_err(serde::de::Error::custom)
                .map(|()| component)
        },
    );
    let component = match component {
        Ok(component) => component,
        Err(error) => {
            reasons.push(format!("replace invalid component metadata with unaccepted stale draft: {error}; component acceptance is required"));
            unaccepted_component()
        }
    };
    let entries = value["tasks"].as_sequence().ok_or(
        "tasks must be a sequence; use an approved --reset to replace an unreadable queue",
    )?;
    let mut tasks = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        if !entry["recovery_state"].is_null() || entry["status"].as_str() == Some("in-progress") {
            return Err(format!(
                "task entry {index} may have active/fenced execution; use `kvist task recover` when evidence permits or approved --clean-slate; effects remain unknown"
            ));
        }
        let id = entry["id"].as_str().unwrap_or("<missing ID>");
        let mut recovered_entry = entry.clone();
        if let Some(fields) = recovered_entry.as_mapping_mut() {
            fields.retain(|key, _| {
                let keep = matches!(key.as_str(), Some(
                    "id" | "title" | "description" | "context" | "purpose" | "expected_outcome"
                    | "kind" | "status" | "depends_on" | "requirements" | "timestamps"
                    | "blocked_reason" | "recovery_state" | "acceptance_id" | "disposition"
                ));
                if !keep {
                    reasons.push(format!("task entry {index} (`{id}`): discard unknown field {key:?}; original retained"));
                }
                keep
            });
        }
        recover_timestamps(&mut recovered_entry, id, reasons);
        match serde_yaml::from_value::<Task>(recovered_entry) {
            Ok(mut task) => {
                reconcile(&mut task, reasons);
                match task_queue::validate_task(&task, true) {
                    Ok(()) => tasks.push(task),
                    Err(error) => {
                        reasons.push(format!("remove task entry {index} (`{id}`): {error}"))
                    }
                }
            }
            Err(error) => reasons.push(format!("remove task entry {index} (`{id}`): {error}")),
        }
    }
    let mut first = BTreeMap::new();
    let mut conflicting = BTreeSet::new();
    for task in &tasks {
        if let Some(previous) = first.get(&task.id) {
            if previous != task {
                conflicting.insert(task.id.clone());
            }
        } else {
            first.insert(task.id.clone(), task.clone());
        }
    }
    let mut retained = BTreeSet::new();
    tasks.retain(|task| {
        if conflicting.contains(&task.id) {
            reasons.push(format!("remove conflicting duplicate task `{}` and leave its dependants unrecoverable; identity cannot be guessed", task.id));
            false
        } else if !retained.insert(task.id.clone()) {
            reasons.push(format!("merge identical recovered task data for duplicate `{}`; retain one identity and its dependency edges", task.id));
            false
        } else {
            true
        }
    });
    Ok(TaskQueue {
        schema_version: 1,
        component,
        tasks,
    })
}

fn recover_timestamps(entry: &mut Value, id: &str, reasons: &mut Vec<String>) {
    let status = entry["status"].as_str().unwrap_or("<unknown>").to_owned();
    let valid = serde_yaml::from_value::<TaskTimestamps>(entry["timestamps"].clone()).is_ok_and(
        |timestamps| {
            timestamps.created_at.validate("created_at").is_ok()
                && timestamps.updated_at.validate("updated_at").is_ok()
                && timestamps
                    .completed_at
                    .as_ref()
                    .is_none_or(|date| date.validate("completed_at").is_ok())
                && (status != "completed" || timestamps.completed_at.is_some())
        },
    );
    if valid {
        return;
    }
    let Some(fields) = entry.as_mapping_mut() else {
        return;
    };
    let prior_reason = fields
        .get(Value::String("blocked_reason".to_owned()))
        .and_then(Value::as_str)
        .filter(|reason| !reason.trim().is_empty())
        .map(|reason| format!(" Original blocker: {reason}"))
        .unwrap_or_default();
    let reason = format!(
        "Recovery: original timestamp history is unknown or invalid (original claimed status: {status}); epoch timestamps are synthetic unknowns, not observed dates or completion evidence. Review the backup before explicit resumption.{prior_reason}"
    );
    fields.insert(
        Value::String("status".to_owned()),
        Value::String("blocked".to_owned()),
    );
    fields.insert(
        Value::String("blocked_reason".to_owned()),
        Value::String(reason),
    );
    for field in ["acceptance_id", "disposition"] {
        if fields
            .get(Value::String(field.to_owned()))
            .is_some_and(|value| !value.is_null())
        {
            fields.insert(Value::String(field.to_owned()), Value::Null);
            reasons.push(format!("task `{id}` {field} -> null: unknown timestamp history cannot assert prior finalization authority; original reference retained in backup"));
        }
    }
    let mut timestamps = serde_yaml::Mapping::new();
    for field in ["created_at", "updated_at"] {
        timestamps.insert(
            Value::String(field.to_owned()),
            Value::String(Timestamp::default_instant().to_string()),
        );
    }
    timestamps.insert(Value::String("completed_at".to_owned()), Value::Null);
    timestamps.insert(Value::String("recovered".to_owned()), Value::Bool(true));
    fields.insert(
        Value::String("timestamps".to_owned()),
        Value::Mapping(timestamps),
    );
    reasons.push(format!(
        "retain task `{id}` and its dependency edges: missing/invalid timestamps; status {status} -> blocked, completed_at -> null, created_at/updated_at -> synthetic epoch, timestamps.recovered -> true; original history is unknown and preserved in the backup, not reconstructed"
    ));
}

fn reconcile(task: &mut Task, reasons: &mut Vec<String>) {
    for (name, value) in [
        ("title", &mut task.title),
        ("description", &mut task.description),
        ("context", &mut task.context),
        ("purpose", &mut task.purpose),
        ("expected_outcome", &mut task.expected_outcome),
    ] {
        if value.as_str() != value.trim() {
            reasons.push(format!(
                "task `{}` {name}: {value:?} -> {:?} (trim surrounding whitespace)",
                task.id,
                value.trim()
            ));
            *value = value.trim().to_owned();
        }
    }
    let timestamps = &mut task.timestamps;
    if timestamps.created_at.validate("created_at").is_ok()
        && timestamps.updated_at.validate("updated_at").is_ok()
        && timestamps.updated_at < timestamps.created_at
    {
        reasons.push(format!("task `{}` updated_at: {} -> {} (assume creation time is authoritative; original retained)", task.id, timestamps.updated_at, timestamps.created_at));
        timestamps.updated_at = timestamps.created_at.clone();
    }
    if task.status == TaskStatus::Completed {
        if let Some(completed) = &mut timestamps.completed_at
            && completed.validate("completed_at").is_ok()
            && timestamps.updated_at.validate("updated_at").is_ok()
            && *completed < timestamps.updated_at
        {
            reasons.push(format!("task `{}` completed_at: {completed} -> {} (assume latest update is authoritative; this reconciles history, not new completion evidence)", task.id, timestamps.updated_at));
            *completed = timestamps.updated_at.clone();
        }
    } else if timestamps.completed_at.take().is_some() {
        reasons.push(format!("task `{}` completed_at -> null: retain non-completed status; discarded conflicting date remains in backup", task.id));
    }
    if matches!(
        task.status,
        TaskStatus::Blocked | TaskStatus::AwaitingDecision
    ) {
        if task
            .blocked_reason
            .as_deref()
            .is_none_or(|reason| reason.trim().is_empty())
        {
            reasons.push(format!(
                "task `{}` blocked_reason: {:?} -> recovery blocker; original rationale is unknown",
                task.id, task.blocked_reason
            ));
            task.blocked_reason = Some("Recovery: the original blocker reason was missing; inspect the retained backup before resuming.".to_owned());
        }
    } else if let Some(reason) = task.blocked_reason.take() {
        reasons.push(format!(
            "task `{}` blocked_reason: {reason:?} -> null (retain {:?} status)",
            task.id, task.status
        ));
    }
    for values in [&mut task.depends_on, &mut task.requirements] {
        values.sort();
        values.dedup();
    }
}

fn ordered_tasks(tasks: Vec<Task>) -> (Vec<Task>, Vec<Task>) {
    let mut pending = tasks.into_iter().map(Some).collect::<Vec<_>>();
    let mut ordered = Vec::new();
    let mut ids = BTreeSet::new();
    loop {
        let next = pending.iter().position(|task| {
            task.as_ref().is_some_and(|task| {
                !ids.contains(&task.id) && task.depends_on.iter().all(|id| ids.contains(id))
            })
        });
        let Some(index) = next else { break };
        if let Some(task) = pending[index].take() {
            ids.insert(task.id.clone());
            ordered.push(task);
        }
    }
    (ordered, pending.into_iter().flatten().collect())
}
