use std::io::Write;

use anyhow::Result;
use crossterm::{terminal::{enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode}, execute, event::{EnableMouseCapture, DisableMouseCapture}};
use tui::{backend::Backend, Terminal};

pub struct TerminalSettings<B: Backend + Write> {
    terminal: Terminal<B>,
}

impl<B: Backend + Write> TerminalSettings<B> {
    pub fn mangle_terminal<W, F>(mut stream: W, backend: F) -> Result<Self>
    where
        W: Write,
        F: Fn(W) -> B,
    {
        enable_raw_mode()?;
        execute!(stream, EnterAlternateScreen, EnableMouseCapture)?;
        let terminal = Terminal::new(backend(stream))?;

        Ok(
            Self {
                terminal,
            }
        )
    }

    pub fn terminal_mut(&mut self) -> &mut Terminal<B> {
        &mut self.terminal
    }
}

impl<B: Backend + Write> Drop for TerminalSettings<B> {
    fn drop(&mut self) {
        disable_raw_mode().unwrap();
        execute!(self.terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture).unwrap();
        self.terminal.show_cursor().unwrap();
    }
}
