use skott::{Config, RunLimits};

fn model(extra: &str) -> (tempfile::TempDir, Config) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        format!(
            "schema_version=1\nworking_directory={:?}\ndefault_provider='llama-server'\n\
         [[models]]\nid='test'\nprovider='llama-server'\n\
         base_url='http://127.0.0.1:1'\nmodel='test'\n{extra}",
            dir.path().to_str().unwrap()
        ),
    )
    .unwrap();
    let config = Config::load(&path).unwrap();
    (dir, config)
}

#[test]
fn configured_capacity_and_cli_precedence_have_no_8192_fallback() {
    let (_dir, config) = model("context_limit=262144\nresponse_reserve=16384\n");
    let model = &config.models[0];
    let resolved = model
        .resolve_budgets(None, None, RunLimits::default())
        .unwrap();
    assert_eq!(resolved.context_limit, 262144);
    assert_eq!(resolved.limits.response_reserve, 16384);
    assert_eq!(resolved.context_source, "configuration");
    let overridden = model
        .resolve_budgets(Some(65536), Some(8192), RunLimits::default())
        .unwrap();
    assert_eq!(overridden.context_limit, 65536);
    assert_eq!(overridden.limits.response_reserve, 8192);
    assert_eq!(overridden.context_source, "CLI");
}

#[test]
fn automatic_generation_reserve_has_room_for_reasoning_but_fits_small_windows() {
    let (_dir, config) = model("context_limit=262144\n");
    let model = &config.models[0];
    assert_eq!(
        model
            .resolve_budgets(None, None, RunLimits::default())
            .unwrap()
            .limits
            .response_reserve,
        8192
    );
    assert_eq!(
        model
            .resolve_budgets(Some(4096), None, RunLimits::default())
            .unwrap()
            .limits
            .response_reserve,
        1024
    );
    assert!(
        model
            .resolve_budgets(Some(4096), Some(4096), RunLimits::default())
            .is_err()
    );
}

#[test]
fn unavailable_discovery_requests_explicit_capacity_instead_of_assuming_8192() {
    let (_dir, config) = model("");
    let error = config.models[0]
        .resolve_budgets(None, None, RunLimits::default())
        .unwrap_err();
    assert!(error.describe().contains("context_limit"), "{error}");
}

#[test]
fn provider_capacity_supersedes_both_hints() {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::time::Duration;

    let (_dir, mut config) = model("context_limit=262144\n");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    config.models[0].base_url = format!("http://{}", listener.local_addr().unwrap());
    let worker = std::thread::spawn(move || {
        for (route, body) in [
            ("/models", r#"{"data":[{"id":"test"}]}"#),
            (
                "/props?model=test",
                r#"{"default_generation_settings":{"n_ctx":16384}}"#,
            ),
        ] {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = String::new();
            BufReader::new(&mut stream).read_line(&mut request).unwrap();
            assert!(request.starts_with(&format!("GET {route} ")), "{request}");
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
    });
    let budget = config.models[0]
        .resolve_budgets(Some(4096), None, RunLimits::default())
        .unwrap();
    worker.join().unwrap();
    assert_eq!(
        (budget.context_limit, budget.context_source),
        (16384, "provider")
    );
    assert_eq!(budget.limits.response_reserve, 4096);
}
