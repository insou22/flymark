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
        std::panic::set_hook(Box::new(|panic_info| {
            use crossterm::{terminal, cursor};

            let mut stdout = std::io::stdout();

            execute!(stdout, cursor::MoveTo(0, 0)).unwrap();
            execute!(stdout, terminal::Clear(terminal::ClearType::All)).unwrap();
        
            execute!(stdout, terminal::LeaveAlternateScreen).unwrap();
            execute!(stdout, cursor::Show).unwrap();
        
            terminal::disable_raw_mode().unwrap();

            let panic_handler = better_panic::Settings::auto()
                .create_panic_handler();

            panic_handler(panic_info);
        }));

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

    fn unmangle_terminal(&mut self) {
        disable_raw_mode().unwrap();
        execute!(self.terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture).unwrap();
        self.terminal.show_cursor().unwrap();
    }
}

impl<B: Backend + Write> Drop for TerminalSettings<B> {
    fn drop(&mut self) {
        self.unmangle_terminal();
    }
}
