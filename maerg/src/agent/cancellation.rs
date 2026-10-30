use super::*;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

/// Bridges process interrupts only while an attempt owns the cancellation token.
pub(super) struct SignalCancellation {
    stop: mpsc::Sender<()>,
    watcher: Option<JoinHandle<()>>,
}

impl SignalCancellation {
    pub(super) fn start(cancellation: &CancellationToken) -> Result<Self> {
        sav::install_handler();
        let cancellation = cancellation.clone();
        let (stop, receiver) = mpsc::channel();
        let watcher = thread::Builder::new()
            .name("kvist-attempt-cancellation".into())
            .spawn(move || {
                loop {
                    if sav::take_interrupted() {
                        cancellation.cancel();
                        break;
                    }
                    if !matches!(
                        receiver.recv_timeout(Duration::from_millis(25)),
                        Err(mpsc::RecvTimeoutError::Timeout)
                    ) {
                        break;
                    }
                }
            })
            .map_err(|source| KvistError::Io {
                operation: "start attempt cancellation watcher",
                path: PathBuf::from("."),
                source,
            })?;
        Ok(Self {
            stop,
            watcher: Some(watcher),
        })
    }
}

impl Drop for SignalCancellation {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(watcher) = self.watcher.take()
            && watcher.join().is_err()
        {
            tracing::error!("attempt cancellation watcher failed");
        }
    }
}
