use std::{time::Duration, mem};

use anyhow::{Result, anyhow};
use crossterm::{terminal::{enable_raw_mode, EnterAlternateScreen, disable_raw_mode, LeaveAlternateScreen}, execute, event::{EnableMouseCapture, DisableMouseCapture, self, EventStream, Event, KeyEvent, KeyCode, KeyModifiers}};
use futures::{StreamExt, FutureExt};
use futures_timer::Delay;
use tempfile::TempDir;
use tmux_interface::Session;
use tokio::{time::sleep, select};
use tui::{Terminal, backend::{CrosstermBackend, Backend}, widgets::{Block, Borders}, Frame};

use crate::{Args, choices::Choices};

pub struct UiParams<'a> {
    args:         &'a Args,
    endpoint:     &'a str,
    choices:      &'a Choices,
    tmux_session: &'a Session,
    work_dir:     &'a TempDir,
}

impl<'a> UiParams<'a> {
    pub fn new(
        args:         &'a Args,
        endpoint:     &'a str,
        choices:      &'a Choices,
        tmux_session: &'a Session,
        work_dir:     &'a TempDir,
    ) -> Self {
        Self {
            args,
            endpoint,
            choices,
            tmux_session,
            work_dir,
        }
    }
}

pub struct App<'a> {
    params: UiParams<'a>,
    state: AppState,
}

enum AppState {
    Authenticating(AuthenticatingState),
}

enum AuthenticatingState {
    EnteringZid { current_zid: String, },
    EnteringPassword { zid: String, current_password: String, },
    Authenticated { zid: String, password: String },
}

pub async fn launch_app<B: Backend>(terminal: &mut Terminal<B>, mut app: App<'_>) -> Result<()> {
    let mut event_reader = EventStream::new();

    loop {
        let timeout = Delay::new(Duration::from_millis(100)).fuse();
        let event   = event_reader.next().fuse();

        select! {
            _     = timeout => {},
            event = event   => {
                let event: Result<_> = event.ok_or(anyhow!("Couldn't read input from terminal")).into();
                let event = event??;
                
                if should_quit(event) {
                    break;
                }

                match &mut app.state {
                    AppState::Authenticating(state) => {
                        match state {
                            AuthenticatingState::EnteringZid { current_zid: curr_buf }
                            | AuthenticatingState::EnteringPassword { zid: _, current_password: curr_buf } => {
                                match event {
                                    Event::Key(
                                        KeyEvent { code: KeyCode::Char(char), modifiers: KeyModifiers::NONE }
                                    ) => {
                                        curr_buf.push(char);
                                    }
                                    Event::Key(
                                        KeyEvent { code: KeyCode::Enter, modifiers: KeyModifiers::NONE }
                                    ) => {
                                        match state {
                                            AuthenticatingState::EnteringZid { current_zid } => {
                                                let zid = mem::take(current_zid);
                                                *state = AuthenticatingState::EnteringPassword { zid, current_password: String::new() };
                                            }
                                            AuthenticatingState::EnteringPassword { zid, current_password } => {
                                                let zid = mem::take(zid);
                                                let password = mem::take(current_password);
                                                *state = AuthenticatingState::Authenticated { zid, password };
                                            }
                                            _ => unreachable!("cases covered above"),
                                        }
                                    }
                                    Event::Key(
                                        KeyEvent { code: KeyCode::Backspace, modifiers: KeyModifiers::NONE }
                                    ) => {
                                        curr_buf.pop();
                                    }
                                    _ => {}
                                }
                            }
                            AuthenticatingState::Authenticated { zid, password } => {

                            }
                        }
                    }
                }
            }
        }

        terminal.draw(|f| draw(f, &app))?;
    }

    Ok(())
}

fn should_quit(event: Event) -> bool {
    match event {
        Event::Key(key)     => {
            let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

            match key.code {
                KeyCode::Char('q') => {
                    return true;
                }
                KeyCode::Char('c') if ctrl => {
                    return true;
                }
                _ => {}
            }
        }
        _ => {}
    }

    false
}

pub fn draw<B: Backend>(frame: &mut Frame<B>, app: &App) {
    match &app.state {
        AppState::Authenticating(state) => {
            let size = frame.size();
            let block = Block::default()
                .title("Authenticate")
                .borders(Borders::ALL);
                frame.render_widget(block, size);

            let zid = 

            match state {
                AuthenticatingState::EnteringZid { current_zid } => {

                }
                AuthenticatingState::EnteringPassword { zid, current_password } => {

                }
                AuthenticatingState::Authenticated { zid, password } => {

                }
            }
        }
    }
}

pub async fn launch_ui(params: UiParams<'_>) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
    
    let app = App {
        params,
        state: AppState::Authenticating(AuthenticatingState::EnteringZid { current_zid: String::new() }),
    };

    launch_app(&mut terminal, app).await?;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    Ok(())
}
