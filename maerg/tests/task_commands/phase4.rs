use super::*;
use sha2::Digest;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn fixture(mode: &'static str) -> (TempDir, MockGateway, Arc<AtomicUsize>) {
    let project = tempfile::tempdir_in(".").expect("project");
    initialize(project.path()).expect("initialize");
    fs::write(project.path().join("src/TODOS.yaml"), queue()).expect("queue");
    let root = project.path().canonicalize().expect("root");
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let gateway = MockGateway::serve(
        move |_| {
            let index = counted.fetch_add(1, Ordering::SeqCst);
            if mode == "corrections" && index != 2 {
                let passage = (0..100)
                    .map(|word| format!("word{word} "))
                    .collect::<String>();
                return gateway_text_body(&passage.repeat(6));
            }
            if mode == "changes" && index > 0 {
                fs::write(root.join("src/tests/revision"), format!("revision {index}"))
                    .expect("observed changed revision");
            }
            let mut response: serde_json::Value =
                serde_json::from_str(&gateway_text_body(&format!("Authoring complete {index}.")))
                    .expect("response");
            if mode == "tokens" {
                response["usage"] = serde_json::json!({
                    "prompt_tokens":30000,"completion_tokens":100,"total_tokens":30100
                });
            }
            response.to_string()
        },
        std::time::Duration::ZERO,
    );
    let limits = match mode {
        "tokens" => "token_limit = 31000\n",
        "wall" | "wall-clean" => "timeout_seconds = 15\n",
        _ => "",
    };
    fs::write(project.path().join("kvist.toml"), format!(
        "schema_version = 1\ncomponent_root = \"src\"\n[agent.profiles.developer]\n\
         command_template = '{}'\n{limits}\
         [test_policy]\nschema_version = 1\nworking_directory = \"component\"\n\
         environment_allowlist = [\"PATH\"]\ntimeout_seconds = 20\nmax_output_bytes = 1000\n\
         [[test_policy.commands]]\ncomponent = \".\"\ncommand = \"/usr/bin/echo phase4-verification\"\n",
        gateway_command(&gateway.endpoint)
    )).expect("config");
    track_project(&project);
    let runner = fake_galla_path(&project);
    let source = fs::read_to_string(&runner).expect("runner");
    let delay = if matches!(mode, "wall" | "wall-clean") {
        "sleep 16; "
    } else {
        ""
    };
    let exit = if matches!(mode, "wall-clean" | "inventory-clean") {
        0
    } else {
        1
    };
    fs::write(&runner, source.replace(
        "case \"$request\" in",
        &format!("case \"$request\" in\n  *'phase4-verification'*) {delay}printf 'recurring verification failure'; exit {exit} ;;")
    )).expect("verifier");
    assert!(
        run_kvist(&project, &["task", "approve-policy"])
            .status
            .success()
    );
    (project, gateway, calls)
}

fn evidence(project: &TempDir) -> String {
    fs::read_to_string(
        project
            .path()
            .join("src/.kvist-attempts/implement-code.jsonl"),
    )
    .expect("attempt")
}

#[test]
fn phase4_repairs_cannot_replenish_model_tokens() {
    let (project, _gateway, calls) = fixture("tokens");
    let output = run_kvist(&project, &["task", "run", ".", "implement-code"]);
    fs::remove_file(fake_galla_path(&project)).expect("clean runner");
    assert!(!output.status.success());
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "a repair received a fresh model allowance"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("recurring verification failure"),
        "{stderr}"
    );
    assert!(stderr.contains("model charge"), "{stderr}");
    assert!(stderr.contains("remaining"), "{stderr}");
}

#[test]
fn phase4_verification_time_consumes_original_sequence_deadline() {
    let (project, _gateway, calls) = fixture("wall");
    let output = run_kvist(&project, &["task", "run", ".", "implement-code"]);
    fs::remove_file(fake_galla_path(&project)).expect("clean runner");
    assert!(!output.status.success());
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "verification elapsed time was refunded"
    );
    assert!(
        evidence(&project).contains("\"timed_out\":false"),
        "verification retained its independent ceiling"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("elapsed"), "{stderr}");
    assert!(
        stderr.contains("recurring verification failure"),
        "{stderr}"
    );
}

#[test]
fn phase4_stall_corrections_are_owned_by_the_whole_sequence() {
    let (project, _gateway, calls) = fixture("corrections");
    let output = run_kvist(&project, &["task", "run", ".", "implement-code"]);
    fs::remove_file(fake_galla_path(&project)).expect("clean runner");
    assert!(!output.status.success());
    assert_eq!(
        calls.load(Ordering::SeqCst),
        4,
        "repair reset the two spent corrections"
    );
    assert!(evidence(&project).contains("recurring verification failure"));
    assert!(evidence(&project).contains("\"stall_corrections_used\":2"));
    assert!(evidence(&project).contains("\"remaining_stall_corrections\":0"));
}

#[test]
fn phase4_complete_unchanged_repair_reverifies_once_then_stops_recurring_evidence() {
    let (project, _gateway, calls) = fixture("unchanged");
    let output = run_kvist(&project, &["task", "run", ".", "implement-code"]);
    fs::remove_file(fake_galla_path(&project)).expect("clean runner");
    assert!(!output.status.success());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        evidence(&project)
            .matches("\"phase\":\"verification\"")
            .count(),
        2,
        "an explicit complete repair deserves reverification, not another model grant"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("identical verification"), "{stderr}");
    assert!(stderr.contains("reconcile"), "{stderr}");
}

#[test]
fn phase4_changed_revisions_allow_four_repairs_without_new_resources() {
    let (project, _gateway, calls) = fixture("changes");
    let output = run_kvist(&project, &["task", "run", ".", "implement-code"]);
    fs::remove_file(fake_galla_path(&project)).expect("clean runner");
    assert!(!output.status.success());
    assert_eq!(calls.load(Ordering::SeqCst), 5);
    assert_eq!(
        evidence(&project)
            .matches("\"phase\":\"verification\"")
            .count(),
        5
    );
    assert_eq!(
        evidence(&project)
            .matches("\"phase\":\"verification-repair\"")
            .count(),
        4
    );
    assert!(
        evidence(&project).contains("\"returned_attempts\":5"),
        "final cumulative provenance missing"
    );
    assert!(evidence(&project).contains("\"provenance_complete\":false"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("four repair rounds"), "{stderr}");
    let recorded = evidence(&project);
    let paths = recorded
        .lines()
        .filter_map(|line| {
            let value: serde_json::Value = serde_json::from_str(line).expect("record");
            value["log_path"].as_str().map(str::to_owned)
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        paths.len(),
        5,
        "all authoring/repair effect logs must be linked to the attempt"
    );
    assert!(
        paths
            .iter()
            .all(|path| std::path::Path::new(path).is_file())
    );
}

#[test]
fn phase4_clean_verification_cannot_relabel_an_expired_sequence_completed() {
    let (project, _gateway, calls) = fixture("wall-clean");
    let output = run_kvist(&project, &["task", "run", ".", "implement-code"]);
    fs::remove_file(fake_galla_path(&project)).expect("clean runner");
    assert!(
        !output.status.success(),
        "clean verification refunded elapsed sequence time"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(!evidence(&project).contains("\"phase\":\"verification-finished\""));
    assert!(String::from_utf8_lossy(&output.stderr).contains("clean verification completed after"));
}

#[test]
fn phase4_final_inventory_success_records_only_local_shallow_test_files() {
    let (project, _gateway, calls) = fixture("inventory-clean");
    fs::create_dir_all(project.path().join("src/tests/nested")).unwrap();
    fs::write(project.path().join("src/tests/b.rs"), "second").unwrap();
    fs::write(project.path().join("src/tests/a.rs"), "first").unwrap();
    fs::write(
        project.path().join("src/tests/nested/excluded.rs"),
        "not inventoried",
    )
    .unwrap();
    let output = run_kvist(&project, &["task", "run", ".", "implement-code"]);
    fs::remove_file(fake_galla_path(&project)).expect("clean exact owned runner");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let recorded = evidence(&project);
    let finished = recorded
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("record"))
        .find(|value| value["phase"] == "execution-finished")
        .expect("final change evidence");
    assert_eq!(
        finished["changes"],
        serde_json::json!([
            {"path":"src/./tests/a.rs","operation":"create","pre_digest":null,
                "post_digest":format!("sha256:{}", hex::encode(sha2::Sha256::digest(b"first")))},
            {"path":"src/./tests/b.rs","operation":"create","pre_digest":null,
                "post_digest":format!("sha256:{}", hex::encode(sha2::Sha256::digest(b"second")))},
        ])
    );
    assert!(recorded.contains("\"phase\":\"verification-finished\""));
    assert!(
        project
            .path()
            .join("src/tests/nested/excluded.rs")
            .is_file()
    );
}
