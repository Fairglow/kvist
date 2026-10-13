//! Tests for active-model-first model selection (RUN-REQ-MODEL-SELECTION).
//!
//! These pin the documented precedence: an explicit `--model`/Tab selection is
//! never overridden; the default provider's already-loaded model is selected
//! with no switch (matched by provider model name); a single default-provider
//! model auto-selects; the default provider's default model is used as a
//! fall-back when the provider reports no active model; and several
//! default-provider models with no active match and no single default defer
//! (TUI) or fail fast (headless).

use skott::ModelProvider;
use skott::config::Select;
use skott::config::{
    Config, Model, probe_active_models, select_active_model, unconfigured_active_models,
};
use skott::tools::ToolPolicy;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;

fn model(id: &str, provider_model: &str, base_url: &str) -> Model {
    model_on(
        id,
        provider_model,
        base_url,
        ModelProvider::LlamaServer,
        false,
    )
}

fn model_on(
    id: &str,
    provider_model: &str,
    base_url: &str,
    provider: ModelProvider,
    is_default: bool,
) -> Model {
    Model {
        id: id.to_owned(),
        provider,
        base_url: base_url.to_owned(),
        model: provider_model.to_owned(),
        is_default,
        context_limit: Some(32768),
        response_reserve: None,
        deadline_secs: 300,
        max_attempts: 1,
        retry_base_delay_secs: 1,
        retry_max_delay_secs: 1,
        cadence_timeout_secs: 30,
    }
}

/// A model on the default provider marked as that provider's default.
fn default_model_on(
    id: &str,
    provider_model: &str,
    base_url: &str,
    provider: ModelProvider,
) -> Model {
    model_on(id, provider_model, base_url, provider, true)
}

fn config_with(default_provider: ModelProvider, models: Vec<Model>) -> Config {
    Config::from_parts(
        PathBuf::from("/tmp"),
        default_provider,
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
    let config = config_with(ModelProvider::LlamaServer, vec![a, b]);
    // The provider has Model-B loaded, but the user explicitly chose "a".
    let active = BTreeMap::from([("http://127.0.0.1:9931".to_owned(), "Model-B".to_owned())]);
    let selection = select_active_model(&config, Some("a"), &active);
    assert_eq!(selection, Select::Explicit("a".to_owned()));
}

// --- Rule 2: the default provider's loaded model is used with no switch. ---

#[test]
fn active_model_match_beats_default() {
    let a = model("a", "Model-A", "http://127.0.0.1:9931");
    let b = model("b", "Model-B", "http://127.0.0.1:9931");
    // No default model is marked, but the provider already has Model-B loaded:
    // the active model must be selected so no load/switch is paid.
    let config = config_with(ModelProvider::LlamaServer, vec![a, b]);
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
    let one = model(
        "user-facing-id",
        "Swift-Qwen3.8-27B",
        "http://127.0.0.1:9931",
    );
    let other = model(
        "tiel-coder-35b-q4",
        "Tiel-Coder-35B-A3B",
        "http://127.0.0.1:9931",
    );
    let config = config_with(ModelProvider::LlamaServer, vec![one, other]);
    // The provider has the Tiel model loaded (matched by its provider name).
    let active = BTreeMap::from([(
        "http://127.0.0.1:9931".to_owned(),
        "Tiel-Coder-35B-A3B".to_owned(),
    )]);
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
    let config = config_with(ModelProvider::LlamaServer, vec![a, b, c]);
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

#[test]
fn active_model_on_other_provider_is_not_reused() {
    // The active model is on a non-default provider; the default provider has
    // no active model, so it falls to the default-provider default model.
    let llama = model("llama", "Model-LLa", "http://127.0.0.1:9931");
    let ollama = model_on(
        "ollama",
        "Model-Ollama",
        "http://127.0.0.1:11434",
        ModelProvider::Ollama,
        false,
    );
    let config = config_with(ModelProvider::LlamaServer, vec![llama, ollama]);
    let active = BTreeMap::from([(
        "http://127.0.0.1:11434".to_owned(),
        "Model-Ollama".to_owned(),
    )]);
    // llama is the single default-provider model, so it auto-selects.
    let selection = select_active_model(&config, None, &active);
    assert_eq!(
        selection,
        Select::Single {
            model_id: "llama".to_owned()
        }
    );
}

// --- Rule 3: exactly one default-provider model auto-selects. ---

#[test]
fn single_configured_model_auto_selects() {
    let only = model("only", "Model-Only", "http://127.0.0.1:9931");
    let config = config_with(ModelProvider::LlamaServer, vec![only]);
    // Even with nothing loaded, the single model is chosen (no prompt needed).
    let selection = select_active_model(&config, None, &none_active());
    assert_eq!(
        selection,
        Select::Single {
            model_id: "only".to_owned()
        }
    );
}

#[test]
fn single_default_provider_model_auto_selects_among_others() {
    // The default provider (llama-server) has exactly one model; an ollama
    // model is also configured. The default provider's single model is chosen.
    let llama = model("llama", "Model-LLa", "http://127.0.0.1:9931");
    let ollama = model_on(
        "ollama",
        "Model-Ollama",
        "http://127.0.0.1:11434",
        ModelProvider::Ollama,
        false,
    );
    let config = config_with(ModelProvider::LlamaServer, vec![llama, ollama]);
    let selection = select_active_model(&config, None, &none_active());
    assert_eq!(
        selection,
        Select::Single {
            model_id: "llama".to_owned()
        }
    );
}

// --- Rule 4: several default-provider models, no active match. ---

#[test]
fn several_models_no_active_match_uses_default_model() {
    let a = model("a", "Model-A", "http://127.0.0.1:9931");
    let b = default_model_on(
        "b",
        "Model-B",
        "http://127.0.0.1:9931",
        ModelProvider::LlamaServer,
    );
    let config = config_with(ModelProvider::LlamaServer, vec![a, b]);
    // Provider down: no active model to match. The default-provider default is
    // used as the fall-back (it may need to be loaded).
    let selection = select_active_model(&config, None, &none_active());
    assert_eq!(
        selection,
        Select::Default {
            model_id: "b".to_owned(),
            reason: "no active model detected; using default model `b` for `llama-server`"
                .to_owned(),
            loaded: "Model-B".to_owned(),
        }
    );
}

#[test]
fn several_models_no_active_match_no_default_needs_selection() {
    let a = model("a", "Model-A", "http://127.0.0.1:9931");
    let b = model("b", "Model-B", "http://127.0.0.1:9931");
    let config = config_with(ModelProvider::LlamaServer, vec![a, b]);
    // Provider down and no default model marked: defer (TUI) / fail (headless).
    let selection = select_active_model(&config, None, &none_active());
    assert_eq!(selection, Select::NeedsSelection);
}

#[test]
fn several_models_active_model_not_configured_uses_default() {
    let a = model("a", "Model-A", "http://127.0.0.1:9931");
    let b = default_model_on(
        "b",
        "Model-B",
        "http://127.0.0.1:9931",
        ModelProvider::LlamaServer,
    );
    let config = config_with(ModelProvider::LlamaServer, vec![a, b]);
    // The provider has a model that is not in [[models]]: no active match, so
    // the default model is the fall-back.
    let active = BTreeMap::from([(
        "http://127.0.0.1:9931".to_owned(),
        "Model-Unknown".to_owned(),
    )]);
    let selection = select_active_model(&config, None, &active);
    assert_eq!(
        selection,
        Select::Default {
            model_id: "b".to_owned(),
            reason: "no active model detected; using default model `b` for `llama-server`"
                .to_owned(),
            loaded: "Model-B".to_owned(),
        }
    );
}

// --- The active model is never overridden by the default model. ---

#[test]
fn active_model_is_used_not_default() {
    // This is the reported scenario: the default model is "a", but the
    // provider already has a *different* configured model loaded. The active
    // model must be used with no switch, not the default.
    let a = default_model_on(
        "a",
        "Model-A",
        "http://127.0.0.1:9931",
        ModelProvider::LlamaServer,
    );
    let b = model("b", "Model-B", "http://127.0.0.1:9931");
    let config = config_with(ModelProvider::LlamaServer, vec![a, b]);
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
fn default_model_is_never_used_when_single_model() {
    // A single configured model auto-selects regardless of is_default.
    let a = default_model_on(
        "a",
        "Model-A",
        "http://127.0.0.1:9931",
        ModelProvider::LlamaServer,
    );
    let config = config_with(ModelProvider::LlamaServer, vec![a]);
    let selection = select_active_model(&config, None, &none_active());
    assert_eq!(
        selection,
        Select::Single {
            model_id: "a".to_owned()
        }
    );
}

#[test]
fn probe_reports_loaded_model_without_inference() {
    let (base_url, requested) =
        run_llama_props_mock(r#"{"model":"Tiel-Coder-35B-A3B-MTP-UD-Q4_K_XL"}"#.to_owned());
    // Two configured models on the same endpoint force the probe to run (a
    // single model auto-selects without probing). The probe dedups by base_url,
    // so the server sees exactly one request.
    let tiel = model("tiel", "Tiel-Coder-35B-A3B-MTP-UD-Q4_K_XL", &base_url);
    let other = model("other", "Some-Other-Model", &base_url);
    let config = config_with(ModelProvider::LlamaServer, vec![tiel, other]);
    let active = probe_active_models(&config);
    assert_eq!(
        active,
        BTreeMap::from([(
            base_url.clone(),
            "Tiel-Coder-35B-A3B-MTP-UD-Q4_K_XL".to_owned()
        )])
    );
    // The probe is a bare GET /props (no model-qualified switch, no inference).
    let requests = requested.lock().unwrap().clone();
    assert_eq!(
        requests.len(),
        1,
        "probe must hit the endpoint once: {requests:?}"
    );
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
    let config = config_with(ModelProvider::LlamaServer, vec![m]);
    assert!(
        probe_active_models(&config).is_empty(),
        "no probe for a single model"
    );
}

#[test]
fn probe_falls_through_when_provider_is_down() {
    // A down provider (nothing listening) yields no entry and does not fail the
    // probe, so selection can fall through to the next rule. Two configured
    // models (same down endpoint) force the probe to attempt the connection.
    let down = "http://127.0.0.1:1";
    let a = model("a", "Model-A", down);
    let b = model("b", "Model-B", down);
    let config = config_with(ModelProvider::LlamaServer, vec![a, b]);
    let active = probe_active_models(&config);
    assert!(
        active.is_empty(),
        "down provider must yield no active entry"
    );
}

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
fn unconfigured_active_model_is_reported() {
    let config = config_with(
        ModelProvider::LlamaServer,
        vec![model("a", "Model-A", "http://127.0.0.1:9931")],
    );
    let active = BTreeMap::from([(
        "http://127.0.0.1:9931".to_owned(),
        "Model-Unknown".to_owned(),
    )]);
    let unconfigured = unconfigured_active_models(&config, &active);
    assert_eq!(
        unconfigured,
        vec![(
            "Model-Unknown".to_owned(),
            "http://127.0.0.1:9931".to_owned()
        )]
    );
}
