use super::*;

fn edit(path: &str, before: &str, after: &str) -> DocumentEdit {
    DocumentEdit {
        path: path.to_owned(),
        before: before.to_owned(),
        after: after.to_owned(),
    }
}

fn draft(fixture: &Fixture, edits: Vec<DocumentEdit>) -> Draft {
    let mut draft = fixture.draft();
    draft.documents.clear();
    draft.document_edits = edits;
    draft
}

fn append(fixture: &Fixture, text: &str) -> String {
    let path = fixture.component().join("DESIGN.md");
    let contents = format!("{}\n{text}\n", fs::read_to_string(&path).unwrap());
    fs::write(path, &contents).unwrap();
    contents
}

#[test]
fn compact_unique_edit_freezes_and_preserves_large_unchanged_content() {
    let fixture = Fixture::new();
    let padding = "Unchanged local paragraph: é λ 東京.\n".repeat(6_000);
    let before = append(
        &fixture,
        &format!("{padding}\nUnique decision anchor: original."),
    );
    assert!(before.len() > 200 * 1024);
    let draft = draft(
        &fixture,
        vec![edit(
            "DESIGN.md",
            "Unique decision anchor: original.",
            "Unique decision anchor: approved.",
        )],
    );
    assert!(encode(&draft).unwrap().len() < 8192);
    let plan = fixture.prepare(draft).unwrap();
    let after = before.replacen(
        "Unique decision anchor: original.",
        "Unique decision anchor: approved.",
        1,
    );
    assert_eq!(plan.input.files["DESIGN.md"], before);
    assert_eq!(plan.outputs["DESIGN.md"], after);
    fixture.apply(&plan).unwrap();
    assert_eq!(
        fs::read_to_string(fixture.component().join("DESIGN.md")).unwrap(),
        after
    );
}

#[test]
fn missing_empty_multiple_and_overlapping_anchors_are_rejected() {
    for before in [
        "",
        "never-present-exact-anchor",
        "duplicate-exact-anchor",
        "aa",
    ] {
        let fixture = Fixture::new();
        append(
            &fixture,
            "duplicate-exact-anchor duplicate-exact-anchor aaa",
        );
        let original = fixture.input().files;
        assert!(
            fixture
                .prepare(draft(
                    &fixture,
                    vec![edit("DESIGN.md", before, "replacement")]
                ))
                .is_err()
        );
        assert_eq!(fixture.input().files, original);
    }
}

#[test]
fn full_replacement_overlap_and_nonlocal_paths_are_rejected() {
    let fixture = Fixture::new();
    append(&fixture, "A unique exact scope anchor.");
    let mut overlapping = fixture.draft();
    overlapping
        .document_edits
        .push(edit("DESIGN.md", "A unique exact scope anchor.", "changed"));
    assert!(fixture.prepare(overlapping).is_err());
    for path in [
        "../DESIGN.md",
        "./DESIGN.md",
        "/DESIGN.md",
        "src/lib.rs",
        "TODOS.yaml",
        "IMPL.md",
    ] {
        assert!(
            fixture
                .prepare(draft(&fixture, vec![edit(path, "anchor", "replacement")]))
                .is_err()
        );
    }
}

#[test]
fn sequential_literal_edits_match_unicode_and_newly_created_anchors() {
    let fixture = Fixture::new();
    let before = append(&fixture, "État: café 🦀; 東京 old.");
    let plan = fixture
        .prepare(draft(
            &fixture,
            vec![
                edit("DESIGN.md", "café 🦀", "naïve λ"),
                edit("DESIGN.md", "naïve λ; 東京 old.", "naïve λ; 東京 approved."),
            ],
        ))
        .unwrap();
    let after = before.replace("État: café 🦀; 東京 old.", "État: naïve λ; 東京 approved.");
    assert_eq!(plan.outputs["DESIGN.md"], after);
    fixture.apply(&plan).unwrap();
    assert_eq!(
        fs::read_to_string(fixture.component().join("DESIGN.md")).unwrap(),
        after
    );
}

#[test]
fn document_edit_count_and_complete_candidate_bounds_are_enforced() {
    let fixture = Fixture::new();
    append(&fixture, "Count-bound unique anchor.");
    let no_op = edit(
        "DESIGN.md",
        "Count-bound unique anchor.",
        "Count-bound unique anchor.",
    );
    assert!(
        fixture
            .prepare(draft(&fixture, vec![no_op.clone(); 128]))
            .is_ok()
    );
    assert!(fixture.prepare(draft(&fixture, vec![no_op; 129])).is_err());
    let fixture = Fixture::new();
    append(
        &fixture,
        &format!(
            "{}\nExpansion-bound unique anchor.",
            "x".repeat(MAX_FILE as usize / 2)
        ),
    );
    let expansion = draft(
        &fixture,
        vec![edit(
            "DESIGN.md",
            "Expansion-bound unique anchor.",
            &"y".repeat(MAX_FILE as usize / 2),
        )],
    );
    assert!(encode(&expansion).unwrap().len() < MAX_FILE as usize);
    assert!(fixture.prepare(expansion).is_err());
}

#[test]
fn optional_edits_default_empty_and_reject_untyped_fields() {
    let parsed: Draft = serde_json::from_value(serde_json::json!({
        "schema_version":1, "rationale":"Manual unchanged-intent retry",
        "documents":{}, "tasks":[], "disposition":"resolve",
    }))
    .unwrap();
    assert!(parsed.document_edits.is_empty());
    assert!(
        serde_json::from_value::<DocumentEdit>(serde_json::json!({
            "path":"DESIGN.md", "before":"anchor", "after":"replacement", "regex":true,
        }))
        .is_err()
    );
}
