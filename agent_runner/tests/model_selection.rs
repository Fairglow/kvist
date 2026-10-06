//! Tests for active-model-first model selection (RUN-REQ-MODEL-SELECTION).
//!
//! These pin the documented precedence before the production selection path
//! exists: an explicit `--model`/Tab selection is never overridden; a loaded
//! provider model matching a configured entry's provider model name is selected
//! with no switch; exactly one configured model auto-selects; several configured
//! models with no active match defer (TUI) or fail fast (headless); and
//! `default_model` is only a last resort that never overrides an active model,
//! an explicit selection, or a single model.

use agent_runner::config::{select_active_model, unconfigured_active_models, Select};
use agent_runner::config::{probe_active_models, Config, Model};
use agent_runner::tools::ToolPolicy;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;

fn model(id: &str, provider_model: &str, base_url: &str) -> Model {
    Model {
        id: id.to_owned(),
        provider: agent_runner::ModelProvider::LlamaServer,
        base_url: base_url.to_owned(),
        model: provider_model.to_owned(),
        context_limit: Some(32768),
        response_reserve: None,
        deadline_secs: 300,
        max_attempts: 1,
        retry_base_delay_secs: 1,
        retry_max_delay_secs: 1,
        cadence_timeout_secs: 30,
    }
}

fn config_with(default: &str, models: Vec<Model>) -> Config {
    Config::from_parts(
        PathBuf::from("/tmp"),
        default,
        models,
        ToolPolicy::minimum(),
    )
    .unwrap()
}

/// No provider is up: the active map is empty.
fn none_active() -> BTreeMap<String, String> {
    BTreeMap::new()
}

// --- Rule 1: an explicit selection always wins, even over an active model. ---

#[test]
fn explicit_selection_wins_over_active_model() {
    let a = model("a", "Model-A", "http://127.0.0.1:9931");
    let b = model("b", "Model-B", "http://127.0.0.1:9931");
    let config = config_with("a", vec![a, b]);
    // The provider has Model-B loaded, but the user explicitly chose "a".
    let active = BTreeMap::from([("http://127.0.0.1:9931".to_owned(), "Model-B".to_owned())]);
    let selection = select_active_model(&config, Some("a"), &active);
    assert_eq!(selection, Select::Explicit("a".to_owned()));
}

// --- Rule 2: a loaded model matching a configured entry is used with no switch. ---

#[test]
fn active_model_match_beats_default() {
    let a = model("a", "Model-A", "http://127.0.0.1:9931");
    let b = model("b", "Model-B", "http://127.0.0.1:9931");
    // default_model is "a", but the provider already has Model-B loaded: the
    // active model must be selected so no load/switch is paid.
    let config = config_with("a", vec![a, b]);
    let active = BTreeMap::from([("http://127.0.0.1:9931".to_owned(), "Model-B".to_owned())]);
    let selection = select_active_model(&config, None, &active);
    assert_eq!(
        selection,
        Select::Active {
            model_id: "b".to_owned(),
            reason: "using already-loaded model `Model-B` (no switch)".to_owned(),
            loaded: "Model-B".to_owned(),
        }
    );
}

#[test]
fn active_match_is_by_provider_model_name_not_id() {
    // The configured `id` and the provider-facing `model` differ; matching must
    // be on the provider-facing name.
    let one = model("user-facing-id", "Swift-Qwen3.8-27B", "http://127.0.0.1:9931");
    let other = model("tiel-coder-35b-q4", "Tiel-Coder-35B-A3B", "http://127.0.0.1:9931");
    let config = config_with("user-facing-id", vec![one, other]);
    // The provider has the Tiel model loaded (matched by its provider name).
    let active = BTreeMap::from([
        ("http://127.0.0.1:9931".to_owned(), "Tiel-Coder-35B-A3B".to_owned()),
    ]);
    let selection = select_active_model(&config, None, &active);
    assert_eq!(
        selection,
        Select::Active {
            model_id: "tiel-coder-35b-q4".to_owned(),
            reason: "using already-loaded model `Tiel-Coder-35B-A3B` (no switch)".to_owned(),
            loaded: "Tiel-Coder-35B-A3B".to_owned(),
        }
    );
}

#[test]
fn single_active_match_among_several_is_selected() {
    let a = model("a", "Model-A", "http://127.0.0.1:9931");
    let b = model("b", "Model-B", "http://127.0.0.1:9932");
    let c = model("c", "Model-C", "http://127.0.0.1:9933");
    let config = config_with("a", vec![a, b, c]);
    // Only one server is up and it has Model-B loaded.
    let active = BTreeMap::from([("http://127.0.0.1:9932".to_owned(), "Model-B".to_owned())]);
    let selection = select_active_model(&config, None, &active);
    assert_eq!(
        selection,
        Select::Active {
            model_id: "b".to_owned(),
            reason: "using already-loaded model `Model-B` (no switch)".to_owned(),
            loaded: "Model-B".to_owned(),
        }
    );
}

// --- Rule 3: exactly one configured model auto-selects. ---

#[test]
fn single_configured_model_auto_selects() {
    let only = model("only", "Model-Only", "http://127.0.0.1:9931");
    let config = config_with("only", vec![only]);
    // Even with nothing loaded, the single model is chosen (no prompt needed).
    let selection = select_active_model(&config, None, &none_active());
    assert_eq!(selection, Select::Single { model_id: "only".to_owned() });
}

// --- Rule 4: several models, no active match -> defer (TUI) / fail (headless). ---

#[test]
fn several_models_no_active_match_needs_selection() {
    let a = model("a", "Model-A", "http://127.0.0.1:9931");
    let b = model("b", "Model-B", "http://127.0.0.1:9931");
    let config = config_with("a", vec![a, b]);
    // Provider down: no active model to match.
    let selection = select_active_model(&config, None, &none_active());
    assert_eq!(selection, Select::NeedsSelection);
}

#[test]
fn several_models_active_model_not_configured_needs_selection() {
    let a = model("a", "Model-A", "http://127.0.0.1:9931");
    let b = model("b", "Model-B", "http://127.0.0.1:9931");
    let config = config_with("a", vec![a, b]);
    // The provider has a model that is not in [[models]].
    let active = BTreeMap::from([("http://127.0.0.1:9931".to_owned(), "Model-Unknown".to_owned())]);
    let selection = select_active_model(&config, None, &active);
    assert_eq!(selection, Select::NeedsSelection);
}

// --- default_model never overrides an active model, explicit, or single. ---

#[test]
fn default_is_last_resort_never_overrides_active() {
    // This is the reported scenario: default_model changed to one model, but the
    // provider already has a *different* configured model loaded. The active
    // model must be used with no switch, not the default.
    let qwen = model("qwen38-27b-q4-swift", "Swift-Qwen3.8-27B-Q4_K_M", "http://127.0.0.1:9931");
    let tiel = model("tiel-coder-35b-q4", "Tiel-Coder-35B-A3B-MTP-UD-Q4_K_XL", "http://127.0.0.1:9931");
    let config = config_with("qwen38-27b-q4-swift", vec![qwen, tiel]);
    let active = BTreeMap::from([(
        "http://127.0.0.1:9931".to_owned(),
        "Tiel-Coder-35B-A3B-MTP-UD-Q4_K_XL".to_owned(),
    )]);
    let selection = select_active_model(&config, None, &active);
    assert_eq!(
        selection,
        Select::Active {
            model_id: "tiel-coder-35b-q4".to_owned(),
            reason: "using already-loaded model `Tiel-Coder-35B-A3B-MTP-UD-Q4_K_XL` (no switch)"
                .to_owned(),
            loaded: "Tiel-Coder-35B-A3B-MTP-UD-Q4_K_XL".to_owned(),
        }
    );
}

// --- Ambiguity: more than one configured entry matches the active model. ---

#[test]
fn multiple_active_matches_are_ambiguous_and_needs_selection() {
    // Two entries share the same provider model name on the same server: the
    // loaded model cannot uniquely identify one of them.
    let a = model("a", "Shared-Model", "http://127.0.0.1:9931");
    let b = model("b", "Shared-Model", "http://127.0.0.1:9931");
    let config = config_with("a", vec![a, b]);
    let active = BTreeMap::from([("http://127.0.0.1:9931".to_owned(), "Shared-Model".to_owned())]);
    let selection = select_active_model(&config, None, &active);
    assert_eq!(selection, Select::NeedsSelection);
}

// --- An unconfigured active model is surfaced (offered) but never auto-selected. ---

#[test]
fn unconfigured_active_model_is_offered_not_selected() {
    let a = model("a", "Model-A", "http://127.0.0.1:9931");
    let b = model("b", "Model-B", "http://127.0.0.1:9931");
    let config = config_with("a", vec![a, b]);
    let active = BTreeMap::from([("http://127.0.0.1:9931".to_owned(), "Model-New".to_owned())]);
    let selection = select_active_model(&config, None, &active);
    assert_eq!(selection, Select::NeedsSelection);
    // The probe context carries the unconfigured active model for the offer.
    let unconfigured = unconfigured_active_models(&config, &active);
    assert_eq!(
        unconfigured,
        vec![("Model-New".to_owned(), "http://127.0.0.1:9931".to_owned())]
    );
}

// --- The probe is bounded, read-only, loopback-only, and falls through on failure. ---

fn run_llama_props_mock(body: String) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let requested = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let rx = requested.clone();
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut header = Vec::new();
        let mut byte = [0];
        while !header.ends_with(b"\r\n\r\n") {
            let _ = socket.read_exact(&mut byte);
            header.push(byte[0]);
        }
        let header = String::from_utf8_lossy(&header).to_string();
        rx.lock().unwrap().push(header);
        socket.write_all(response.as_bytes()).unwrap();
    });
    (format!("http://{}", address), requested)
}

#[test]
fn probe_reports_loaded_model_without_inference() {
    let (base_url, requested) = run_llama_props_mock(
        r#"{"model":"Tiel-Coder-35B-A3B-MTP-UD-Q4_K_XL"}"#.to_owned(),
    );
    // Two configured models on the same endpoint force the probe to run (a
    // single model auto-selects without probing). The probe dedups by base_url,
    // so the server sees exactly one request.
    let tiel = model("tiel", "Tiel-Coder-35B-A3B-MTP-UD-Q4_K_XL", &base_url);
    let other = model("other", "Some-Other-Model", &base_url);
    let config = config_with("tiel", vec![tiel, other]);
    let active = probe_active_models(&config);
    assert_eq!(
        active,
        BTreeMap::from([(base_url.clone(), "Tiel-Coder-35B-A3B-MTP-UD-Q4_K_XL".to_owned())])
    );
    // The probe is a bare GET /props (no model-qualified switch, no inference).
    let requests = requested.lock().unwrap().clone();
    assert_eq!(requests.len(), 1, "probe must hit the endpoint once: {requests:?}");
    assert!(
        requests[0].starts_with("GET /props"),
        "probe must GET /props: {}",
        requests[0]
    );
}

#[test]
fn single_model_needs_no_active_probe() {
    // A single configured model auto-selects without any provider I/O, so a
    // single-model startup keeps its request sequence (first request = the turn).
    let m = model("only", "Model-Only", "http://127.0.0.1:9999");
    let config = config_with("only", vec![m]);
    assert!(probe_active_models(&config).is_empty(), "no probe for a single model");
}

#[test]
fn probe_falls_through_when_provider_is_down() {
    // A down provider (nothing listening) yields no entry and does not fail the
    // probe, so selection can fall through to the next rule. Two configured
    // models (same down endpoint) force the probe to attempt the connection.
    let down = "http://127.0.0.1:1";
    let a = model("a", "Model-A", down);
    let b = model("b", "Model-B", down);
    let config = config_with("a", vec![a, b]);
    let active = probe_active_models(&config);
    assert!(active.is_empty(), "down provider must yield no active entry");
}
