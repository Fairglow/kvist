use std::io;

pub(crate) fn require_controlling_terminal() -> io::Result<std::fs::File> {
    use std::io::IsTerminal;
    use std::os::unix::fs::OpenOptionsExt;

    let terminal = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOCTTY)
        .open("/dev/tty")
        .and_then(|terminal| {
            if terminal.is_terminal() {
                Ok(terminal)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::NotConnected,
                    "not a terminal",
                ))
            }
        });
    terminal.map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("automatic paging requires a controlling terminal: {error}; use --dump for noninteractive output"),
        )
    })
}

pub(crate) struct TerminalGuard {
    raw: bool,
    alternate: bool,
    hidden: bool,
    mouse: bool,
}

pub(crate) fn wait_event(
    stream: Option<&crate::stream_input::StdinStream>,
    timeout: std::time::Duration,
) -> io::Result<bool> {
    use crossterm::event;
    if event::poll(std::time::Duration::ZERO)? {
        return Ok(true);
    }
    if let Some(stream) = stream.filter(|stream| stream.live()) {
        stream.wait(&io::stdin(), timeout)?;
        event::poll(std::time::Duration::ZERO)
    } else {
        event::poll(timeout)
    }
}

impl TerminalGuard {
    pub fn enter() -> io::Result<Self> {
        let mut guard = Self {
            raw: false,
            alternate: false,
            hidden: false,
            mouse: false,
        };
        crossterm::terminal::enable_raw_mode()?;
        guard.raw = true;
        guard.alternate = true;
        crossterm::execute!(io::stdout(), crossterm::terminal::EnterAlternateScreen)?;
        guard.hidden = true;
        crossterm::execute!(io::stdout(), crossterm::cursor::Hide)?;
        guard.mouse = true;
        crossterm::execute!(io::stdout(), crossterm::event::EnableMouseCapture)?;
        Ok(guard)
    }

    pub fn restore(&mut self) -> io::Result<()> {
        let mut error = None;
        if self.mouse {
            match crossterm::execute!(io::stdout(), crossterm::event::DisableMouseCapture) {
                Ok(()) => self.mouse = false,
                Err(failure) => {
                    error.get_or_insert(failure);
                }
            }
        }
        if self.hidden {
            match crossterm::execute!(io::stdout(), crossterm::cursor::Show) {
                Ok(()) => self.hidden = false,
                Err(failure) => {
                    error.get_or_insert(failure);
                }
            }
        }
        if self.alternate {
            match crossterm::execute!(io::stdout(), crossterm::terminal::LeaveAlternateScreen) {
                Ok(()) => self.alternate = false,
                Err(failure) => {
                    error.get_or_insert(failure);
                }
            }
        }
        if self.raw {
            match crossterm::terminal::disable_raw_mode() {
                Ok(()) => self.raw = false,
                Err(failure) => {
                    error.get_or_insert(failure);
                }
            }
        }
        error.map_or(Ok(()), Err)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if let Err(error) = self.restore() {
            tracing::error!("Failed to restore terminal: {error}");
        }
    }
}
