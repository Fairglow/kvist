//! Owned, cancellable startup jobs; result polling never blocks the editor.

use std::sync::mpsc;
use std::thread::JoinHandle;

use sav::CancellationToken;

use crate::error::{Error, Result};

pub(super) fn update_status(
    app: &mut super::app::App,
    has_worker: bool,
    starting: bool,
    discovering: bool,
) {
    if has_worker {
        return;
    }
    if starting {
        app.status = "starting model…".into();
    } else if discovering {
        app.status = "discovering… (input ready)".into();
    } else if matches!(
        app.status.as_str(),
        "discovering… (input ready)" | "starting model…"
    ) {
        app.status = if app.model.is_some() {
            "ready"
        } else {
            "choose a model (Tab)"
        }
        .into();
    }
}

pub(super) struct Job<T> {
    receiver: mpsc::Receiver<Result<T>>,
    cancellation: CancellationToken,
    thread: Option<JoinHandle<()>>,
}

impl<T: Send + 'static> Job<T> {
    pub(super) fn spawn(
        name: &str,
        work: impl FnOnce(&CancellationToken) -> Result<T> + Send + 'static,
    ) -> Result<Self> {
        let (sender, receiver) = mpsc::channel();
        let cancellation = CancellationToken::new();
        let token = cancellation.clone();
        let thread = std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                let result = work(&token);
                if !token.is_cancelled() {
                    let _ = sender.send(result);
                }
            })
            .map_err(|source| crate::error::io_error("spawn startup job", None, source))?;
        Ok(Self {
            receiver,
            cancellation,
            thread: Some(thread),
        })
    }

    pub(super) fn poll(&self) -> Option<Result<T>> {
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(Error::ChannelClosed)),
        }
    }
}

impl<T> Job<T> {
    pub(super) fn cancel(&self) {
        self.cancellation.cancel();
    }
}

impl<T> Drop for Job<T> {
    fn drop(&mut self) {
        self.cancel();
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("startup worker failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn independent_jobs_do_not_block_polling_or_editor_input() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let (entered, ready) = mpsc::channel();
        let job = Job::spawn("blocked-discovery", move |token| {
            entered.send(()).unwrap();
            while !token.is_cancelled() {
                std::thread::sleep(Duration::from_millis(1));
            }
            Ok(())
        })
        .unwrap();
        ready.recv_timeout(Duration::from_secs(1)).unwrap();
        let other = Job::spawn("independent-discovery", |_| Ok(42)).unwrap();
        let mut app = super::super::app::App::new(
            &["selected".into()],
            Some("selected"),
            sav::ReasoningEffort::Medium,
            None,
            80,
            24,
            super::super::Theme::default(),
        );
        let started = Instant::now();
        assert!(job.poll().is_none());
        app.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(app.editor.lines(), &["x"]);
        loop {
            if let Some(result) = other.poll() {
                assert_eq!(result.unwrap(), 42);
                break;
            }
            assert!(started.elapsed() < Duration::from_secs(1));
            std::thread::yield_now();
        }
        drop(job);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn dropping_provider_discovery_cancels_socket_wait() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let job = Job::spawn("provider-discovery", move |token| {
            sav::DirectModelTransport::new(
                sav::LocalModelProvider::LlamaServer,
                &endpoint,
                Duration::from_secs(60),
                1024,
            )
            .unwrap()
            .loaded_model(token)
            .map_err(|error| Error::ModelTransport {
                model: None,
                reason: error.to_string(),
            })
        })
        .unwrap();
        let (_socket, _) = listener.accept().unwrap();
        let started = Instant::now();
        drop(job);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn prompt_submitted_during_model_discovery_is_staged_and_manual_selection_wins() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = super::super::app::App::new(
            &["a".into(), "b".into()],
            None,
            sav::ReasoningEffort::Medium,
            None,
            80,
            24,
            super::super::Theme::default(),
        );
        app.model_discovery_pending = true;
        app.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL));
        assert_eq!(app.take_pending_prompt().as_deref(), Some("x"));
        assert!(app.editor.is_empty());
        app.model = Some("a".into());
        super::super::apply_selection(
            &mut app,
            crate::config::Select::Active {
                model_id: "b".into(),
                reason: "active".into(),
                loaded: "B".into(),
            },
        );
        assert_eq!(app.model.as_deref(), Some("a"));
        assert!(!app.model_discovery_pending);
    }

    #[test]
    fn overlong_initial_prompt_remains_editable_without_dispatch() {
        let mut app = super::super::app::App::new(
            &["a".into()],
            Some("a"),
            sav::ReasoningEffort::Medium,
            None,
            80,
            24,
            super::super::Theme::default(),
        );
        let text = "x".repeat(1_048_577);
        app.stage_initial_prompt(text.clone());
        assert!(app.take_pending_prompt().is_none());
        assert!(
            app.editor.lines() == [text],
            "overlong text must remain editable"
        );
    }

    #[test]
    fn discovery_status_does_not_overwrite_a_live_worker() {
        let mut app = super::super::app::App::new(
            &["a".into()],
            Some("a"),
            sav::ReasoningEffort::Medium,
            None,
            80,
            24,
            super::super::Theme::default(),
        );
        for status in ["thinking", "done", "cancelled", "error"] {
            app.status = status.into();
            update_status(&mut app, true, false, true);
            assert_eq!(app.status, status);
        }
        update_status(&mut app, false, false, true);
        assert_eq!(app.status, "discovering… (input ready)");
        app.model = None;
        update_status(&mut app, false, false, false);
        assert_eq!(app.status, "choose a model (Tab)");
    }
}
