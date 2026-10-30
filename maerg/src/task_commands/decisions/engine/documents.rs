use super::*;

pub(super) fn materialize(
    captured: &BTreeMap<String, String>,
    replacements: BTreeMap<String, String>,
    edits: Vec<DocumentEdit>,
) -> Result<BTreeMap<String, String>> {
    if edits.len() > 128 {
        return Err(fail(
            Path::new(DIRECTORY),
            "a draft permits at most 128 sequential document edits",
        ));
    }
    let replaced: BTreeSet<_> = replacements.keys().cloned().collect();
    let mut documents = captured.clone();
    for (path, text) in replacements {
        allowed(&path)?;
        bounded(&text, Path::new(&path))?;
        documents.insert(path, text);
        aggregate(&documents)?;
    }
    for edit in edits {
        allowed(&edit.path)?;
        if replaced.contains(&edit.path) {
            return Err(fail(
                Path::new(&edit.path),
                "a document cannot have both a full replacement and literal edits",
            ));
        }
        if edit.before.is_empty() {
            return Err(fail(
                Path::new(&edit.path),
                "document edit anchors must be nonempty",
            ));
        }
        bounded(&edit.before, Path::new(&edit.path))?;
        bounded(&edit.after, Path::new(&edit.path))?;
        let text = documents.get_mut(&edit.path).ok_or_else(|| {
            fail(
                Path::new(&edit.path),
                "edited document is not in the captured local intent",
            )
        })?;
        let start = text.find(&edit.before).ok_or_else(|| {
            fail(
                Path::new(&edit.path),
                "document edit anchor does not match; use an exact current excerpt",
            )
        })?;
        if text.rfind(&edit.before) != Some(start) {
            return Err(fail(
                Path::new(&edit.path),
                "document edit anchor is ambiguous, including overlapping matches; use a unique exact excerpt",
            ));
        }
        let length = text
            .len()
            .checked_sub(edit.before.len())
            .and_then(|length| length.checked_add(edit.after.len()))
            .ok_or_else(|| fail(Path::new(&edit.path), "edited document size is invalid"))?;
        if length as u64 > MAX_FILE {
            return Err(fail(
                Path::new(&edit.path),
                "complete edited document exceeds 1 MiB",
            ));
        }
        text.replace_range(start..start + edit.before.len(), &edit.after);
        aggregate(&documents)?;
    }
    for path in ["REQUIREMENTS.md", "CONTRACT.md", "DESIGN.md"] {
        let text = documents.get(path).ok_or_else(|| {
            fail(
                Path::new(path),
                "local intent document is missing from the capture",
            )
        })?;
        capture::validate_document(path, text)?;
    }
    aggregate(&documents)?;
    Ok(documents)
}

fn allowed(path: &str) -> Result<()> {
    if !matches!(path, "REQUIREMENTS.md" | "CONTRACT.md" | "DESIGN.md") {
        return Err(fail(
            Path::new(DIRECTORY),
            "document changes may target only local REQUIREMENTS.md, CONTRACT.md and DESIGN.md",
        ));
    }
    Ok(())
}

fn aggregate(documents: &BTreeMap<String, String>) -> Result<()> {
    let bytes = documents
        .values()
        .map(String::len)
        .fold(0_usize, usize::saturating_add);
    if bytes > MAX_TOTAL {
        return Err(fail(
            Path::new(DIRECTORY),
            "complete candidate documents exceed 8 MiB",
        ));
    }
    Ok(())
}
