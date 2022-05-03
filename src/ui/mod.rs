mod auth;
mod term;
mod assignments;
mod journals;

use std::time::Duration;

use anyhow::{Result, anyhow};
use crossterm::{event::{EventStream, Event, KeyCode, KeyModifiers}};
use futures::{StreamExt, FutureExt};
use futures_timer::Delay;
use tempfile::TempDir;
use tmux_interface::Session;
use tokio::select;
use tui::{Terminal, backend::{CrosstermBackend, Backend}, Frame};
use tui_input::Input;

use crate::{Args, choices::Choices};

use self::{auth::AuthenticatingState, term::TerminalSettings, assignments::AssignmentsState, journals::JournalsState};

pub struct AppParams<'a> {
    args:         &'a Args,
    endpoint:     &'a str,
    choices:      &'a Choices,
    tmux_session: &'a Session,
    work_dir:     &'a TempDir,
}

impl<'a> AppParams<'a> {
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

pub async fn launch_ui(params: AppParams<'_>) -> Result<()> {
    let mut terminal = TerminalSettings::mangle_terminal(std::io::stdout(), CrosstermBackend::new)?;

    let app = App {
        params,
        state: AppState::Authenticating(AuthenticatingState::EnteringZid { zid_input: Input::default() }),
    };

    launch_app(terminal.terminal_mut(), app).await?;

    Ok(())
}

pub struct App<'a> {
    params: AppParams<'a>,
    state: AppState,
}

pub enum AppState {
    Authenticating(AuthenticatingState),
    Choosing(AssignmentsState),
    Journals(JournalsState),
}

#[derive(Default)]
pub struct UiTickers {
    auth_loading: usize,
}

pub async fn launch_app<B: Backend>(terminal: &mut Terminal<B>, mut app: App<'_>) -> Result<()> {
    let mut ui_tickers   = UiTickers::default();
    let mut event_reader = EventStream::new();

    loop {
        let timeout = Delay::new(Duration::from_millis(10)).fuse();
        let event   = event_reader.next().fuse();

        let event = select! {
            _     = timeout => None,
            event = event   => {
                let event: Result<_> = event.ok_or(anyhow!("Couldn't read input from terminal")).into();
                let event = event??;
                Some(event)
            }
        };
        if let Some(event) = event {
            if should_quit(event) {
                break;
            }
        }

        match &mut app.state {
            AppState::Authenticating(_) => {
                auth::tick_app(&mut app, event)?;
            }
            AppState::Choosing(_) => {
                assignments::tick_app(&mut app, event)?;
            }
            AppState::Journals(_) => {
                journals::tick_app(&mut app, event)?;
            }
        }

        terminal.draw(|f| draw(f, &mut app, &mut ui_tickers))?;
    }

    Ok(())
}

pub fn draw<B: Backend>(frame: &mut Frame<B>, app: &mut App, tickers: &mut UiTickers) {
    match &app.state {
        AppState::Authenticating(_) => {
            auth::draw(frame, app, tickers);
        }
        AppState::Choosing(_) => {
            assignments::draw(frame, app, tickers);
        }
        AppState::Journals(_) => {
            journals::draw(frame, app, tickers);
        }
    }
}

fn should_quit(event: Event) -> bool {
    match event {
        Event::Key(key)     => {
            let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

            match key.code {
                // KeyCode::Char('q') => {
                //     return true;
                // }
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
