//! Advisory review and local receipts for exact component intent bundles.

use super::*;
use decisions::review_storage as store;
use hmac::{KeyInit, Mac};
use subtle::ConstantTimeEq;

const LOCAL: [&str; 3] = ["REQUIREMENTS.md", "CONTRACT.md", "DESIGN.md"];
const MAX_REPORT_BYTES: usize = 64 * 1024;
const MAX_REVIEW_BYTES: usize = 8 * 1024 * 1024;

#[derive(Serialize)]
struct TaskProjection<'a> {
    id: &'a str,
    title: &'a str,
    description: &'a str,
    context: &'a str,
    purpose: &'a str,
    expected_outcome: &'a str,
    kind: TaskKind,
    depends_on: &'a [String],
    requirements: &'a [String],
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    regression_tests: &'a [String],
}

pub(super) fn task_projection(queue: &TaskQueue) -> Result<String> {
    let tasks = queue
        .tasks
        .iter()
        .map(|task| TaskProjection {
            id: &task.id,
            title: &task.title,
            description: &task.description,
            context: &task.context,
            purpose: &task.purpose,
            expected_outcome: &task.expected_outcome,
            kind: task.kind,
            depends_on: &task.depends_on,
            requirements: &task.requirements,
            regression_tests: &task.regression_tests,
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&tasks).map_err(json_error)
}

#[derive(PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleIdentity {
    project: String,
    component: String,
    digests: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema_version: u32,
    review_id: String,
    bundle: BundleIdentity,
    report_digest: String,
    exception: bool,
    authentication: String,
}

fn documents(
    context: &TaskContext,
    config: &crate::config::ProjectConfig,
    local: [&str; 3],
    queue: &TaskQueue,
) -> Result<BTreeMap<String, String>> {
    let mut docs = BTreeMap::new();
    for (name, text) in LOCAL.into_iter().zip(local) {
        docs.insert(name.to_owned(), text.to_owned());
    }
    docs.insert("TASK_PROJECTION".to_owned(), task_projection(queue)?);
    docs.insert(
        "ROOT_CONTRACT.md".to_owned(),
        store::read_text(&context.project_dir.join("ROOT_CONTRACT.md"))?,
    );
    let component_root = context.project_dir.join(&config.component_root);
    if let Some((parent, _)) =
        discovery::find_parent_component(&component_root, &context.component_path)?
    {
        docs.insert(
            "PARENT_CONTRACT.md".to_owned(),
            store::read_text(&parent.join("CONTRACT.md"))?,
        );
    }
    Ok(docs)
}

fn identity(context: &TaskContext, docs: &BTreeMap<String, String>) -> BundleIdentity {
    BundleIdentity {
        project: digest(context.project_dir.as_os_str().as_encoded_bytes()),
        component: digest(context.component_dir.as_os_str().as_encoded_bytes()),
        digests: docs
            .iter()
            .map(|(name, text)| (name.clone(), digest(text.as_bytes())))
            .collect(),
    }
}

fn current_documents(
    context: &TaskContext,
    config: &crate::config::ProjectConfig,
) -> Result<BTreeMap<String, String>> {
    let [requirements, contract, design] =
        LOCAL.map(|name| store::read_text(&context.component_dir.join(name)));
    let queue = read_queue(&context.component_dir)?;
    documents(
        context,
        config,
        [&requirements?, &contract?, &design?],
        &queue,
    )
}

/// Run a tool-free intent review, publishing the reviewed bundle only if it is still current.
pub(crate) fn review_intent(
    context: &TaskContext,
    config: &crate::config::ProjectConfig,
) -> Result<(PathBuf, String)> {
    let docs = current_documents(context, config)?;
    let bundle = identity(context, &docs);
    let evidence = serde_json::to_string(&serde_json::json!({
        "schema_version":1,
        "review_type":"component-intent",
        "component_path":context.component_path,
        "documents":docs,
        "note":"Advisory only. Review intent clarity and coverage, not implementation or compliance."
    })).map_err(json_error)?;
    if evidence.len() > MAX_REVIEW_BYTES {
        return Err(fail("intent review exceeds the 8 MiB context bound"));
    }
    let redactions = evidence_redactions(config);
    let mut session = crate::agent::review::ReviewSession::decision(
        &config.agent.architect,
        &context.component_dir,
        "component-intent-review",
        redactions.clone(),
    )?;
    let result = session.request("decision-intent-review", evidence);
    session.finish(
        if result.is_ok() {
            "completed"
        } else {
            "failed"
        },
        result.as_ref().err(),
    )?;
    let report = redact_bounded(result?.report, &redactions, MAX_REPORT_BYTES);
    let at = clock()?;
    let lock = TaskLock::for_context(context, "component-review", &at)?;
    let result = (|| {
        decisions::ensure_no_pending(&context.component_dir)?;
        let current = crate::config::load(&context.project_dir)?;
        if bundle != identity(context, &current_documents(context, &current)?) {
            return Err(fail("intent changed during review; review the new bundle"));
        }
        lock.revalidate()?;
        publish(context, bundle, &report, false)
    })();
    let released = lock.release();
    let path = result.and_then(|path| released.map(|()| path))?;
    Ok((path, report))
}

pub(super) fn check_acceptance(
    context: &TaskContext,
    config: &crate::config::ProjectConfig,
    queue: &TaskQueue,
    local: [&str; 3],
    acknowledgement: Option<&str>,
    exception: Option<&str>,
) -> Result<()> {
    if !config.review.required && acknowledgement.is_none() && exception.is_none() {
        return Ok(());
    }
    let bundle = identity(context, &documents(context, config, local, queue)?);
    match (acknowledgement, exception) {
        (None, Some(reason)) if !reason.trim().is_empty() && reason.len() <= 8192 => {
            let reason = redact_bounded(reason.to_owned(), &evidence_redactions(config), 8192);
            publish(context, bundle, &reason, true)?;
            Ok(())
        }
        (Some(id), None) => check_receipt(context, &bundle, id),
        (None, None) => Err(fail(&format!(
            "review is required: run `kvist component review {0}` and use its displayed `--review-ack RECEIPT_ID`, or pass `--review-exception REASON`",
            decisions::shell_token(&context.component_path.to_string_lossy())
        ))),
        _ => Err(fail(
            "supply one review acknowledgement or a nonblank review exception (at most 8192 bytes)",
        )),
    }
}

fn clock() -> Result<Timestamp> {
    Timestamp::now().map_err(|source| KvistError::TaskClock { source })
}

fn publish(
    context: &TaskContext,
    bundle: BundleIdentity,
    report: &str,
    exception: bool,
) -> Result<PathBuf> {
    let directory = context.component_dir.join(".kvist/reviews");
    let secret = store::secret(context)?;
    store::directory(&directory)?;
    let mut nonce = [0_u8; 8];
    getrandom::fill(&mut nonce)
        .map_err(|error| fail(&format!("cannot generate review receipt identity: {error}")))?;
    let review_id = format!(
        "{}-{}-{}",
        if exception { "exception" } else { "intent" },
        clock()?.to_string().replace(':', "-"),
        hex::encode(nonce)
    );
    let mut receipt = Receipt {
        schema_version: 2,
        review_id: review_id.clone(),
        bundle,
        report_digest: digest(report.as_bytes()),
        exception,
        authentication: String::new(),
    };
    let unsigned = serde_json::to_vec(&receipt).map_err(json_error)?;
    receipt.authentication = authentication_tag(&secret, &unsigned)?;
    store::write_new(&directory.join(format!("{review_id}.md")), report)?;
    let path = directory.join(format!("{review_id}.receipt.json"));
    store::write_new(
        &path,
        &serde_json::to_string_pretty(&receipt).map_err(json_error)?,
    )?;
    Ok(path)
}

fn check_receipt(context: &TaskContext, bundle: &BundleIdentity, id: &str) -> Result<()> {
    if !id.starts_with("intent-")
        || id.len() > 96
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.'))
        || id.ends_with('.')
    {
        return Err(fail(
            "invalid intent receipt ID; use the ID printed by `kvist component review`",
        ));
    }
    let directory = context.component_dir.join(".kvist/reviews");
    let text = store::read_text(&directory.join(format!("{id}.receipt.json")))?;
    let mut receipt: Receipt = serde_json::from_str(&text).map_err(json_error)?;
    let authentication = std::mem::take(&mut receipt.authentication);
    let payload = serde_json::to_vec(&receipt).map_err(json_error)?;
    let expected = authentication_tag(&store::secret(context)?, &payload)?;
    if receipt.schema_version != 2
        || receipt.exception
        || receipt.review_id != id
        || &receipt.bundle != bundle
        || !bool::from(authentication.as_bytes().ct_eq(expected.as_bytes()))
    {
        return Err(fail(
            "receipt does not authenticate the current intent bundle; run component review again",
        ));
    }
    let report = store::read_text(&directory.join(format!("{id}.md")))?;
    if digest(report.as_bytes()) != receipt.report_digest {
        return Err(fail("review report changed; run component review again"));
    }
    Ok(())
}

fn authentication_tag(secret: &[u8], payload: &[u8]) -> Result<String> {
    let mut tag = hmac::Hmac::<Sha256>::new_from_slice(secret)
        .map_err(|error| fail(&format!("cannot initialize review authentication: {error}")))?;
    tag.update(payload);
    Ok(hex::encode(tag.finalize().into_bytes()))
}

fn fail(reason: &str) -> KvistError {
    KvistError::AgentEvidenceFailed {
        reason: reason.to_owned(),
    }
}

fn json_error(error: serde_json::Error) -> KvistError {
    fail(&format!("invalid review JSON: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_round_trip_binds_exact_bundle_and_report() {
        let project = tempfile::tempdir().unwrap();
        let context = TaskContext {
            project_dir: project.path().to_owned(),
            component_dir: project.path().to_owned(),
            component_path: PathBuf::from("."),
        };
        let mut docs = BTreeMap::from([("REQUIREMENTS.md".into(), "Review me".into())]);
        let bundle = identity(&context, &docs);
        let receipt = publish(
            &context,
            identity(&context, &docs),
            "Advisory feedback",
            false,
        )
        .unwrap();
        let name = receipt.file_name().unwrap().to_str().unwrap();
        let id = name.strip_suffix(".receipt.json").unwrap();
        check_receipt(&context, &bundle, id).unwrap();
        docs.insert("ROOT_CONTRACT.md".into(), "Changed context".into());
        assert!(check_receipt(&context, &identity(&context, &docs), id).is_err());
        fs::write(receipt.with_file_name(format!("{id}.md")), "Edited report").unwrap();
        assert!(check_receipt(&context, &bundle, id).is_err());
    }
}
