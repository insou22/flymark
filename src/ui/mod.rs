mod auth;
mod term;
mod assignments;
mod journals;
mod marking;

use std::time::Duration;

use anyhow::{Result, anyhow};
use crossterm::{event::{EventStream, Event, KeyCode, KeyModifiers}};
use futures::{StreamExt, FutureExt};
use futures_timer::Delay;
use tempfile::TempDir;
use tmux_interface::KillPane;
use tokio::{select, sync::oneshot};
use tui::{Terminal, backend::{CrosstermBackend, Backend}, Frame};
use tui_input::Input;

use crate::choices::Choices;

use self::{auth::AuthenticatingState, term::TerminalSettings, assignments::AssignmentsState, journals::{JournalsState, Journal}, marking::MarkingState};

pub struct AppParams<'a> {
    endpoint:      &'a str,
    choices:       &'a Choices,
    pager_command: &'a str,
    work_dir:      &'a TempDir,
}

impl<'a> AppParams<'a> {
    pub fn new(
        endpoint:     &'a str,
        choices:      &'a Choices,
        pager_command: &'a str,
        work_dir:     &'a TempDir,
    ) -> Self {
        Self {
            endpoint,
            choices,
            pager_command,
            work_dir,
        }
    }
}

pub async fn launch_ui(params: AppParams<'_>) -> Result<()> {
    let mut terminal = TerminalSettings::mangle_terminal(std::io::stdout(), CrosstermBackend::new)?;

    let app = App {
        params,
        auth: None,
        side_pane_id: None,
        state: AppState::Authenticating(AuthenticatingState::EnteringZid { zid_input: Input::default() }),
        mark_puts: Vec::new(),
    };

    launch_app(terminal.terminal_mut(), app).await?;

    Ok(())
}

pub struct App<'a> {
    params: AppParams<'a>,
    auth:  Option<BasicAuth>,
    side_pane_id: Option<String>,
    state: AppState,
    mark_puts: Vec<oneshot::Receiver<Result<()>>>,
}

impl Drop for App<'_> {
    fn drop(&mut self) {
        if let Some(side_pane_id) = self.side_pane_id.as_ref() {
            let _ = KillPane::new()
                .target_pane(side_pane_id)
                .output();
        }
    }
}

#[derive(Debug, Clone)]
pub struct BasicAuth {
    username: String,
    password: String,
}

impl BasicAuth {
    pub fn new(username: String, password: String) -> Self {
        Self {
            username,
            password,
        }
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn password(&self) -> &str {
        &self.password
    }
}

pub enum AppState {
    Authenticating(AuthenticatingState),
    Choosing(AssignmentsState),
    Journals(JournalsState),
    Marking(Vec<Journal>, MarkingState),
}

#[derive(Default)]
pub struct UiTickers {
    auth_loading: usize,
}

pub async fn launch_app<B: Backend>(terminal: &mut Terminal<B>, mut app: App<'_>) -> Result<()> {
    let mut ui_tickers   = UiTickers::default();
    let mut event_reader = EventStream::new();

    let mut trying_to_quit = false;
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

        let mut happy_to_drop = vec![];

        for (i, receiver) in app.mark_puts.iter_mut().enumerate() {
            if let Ok(res) = receiver.try_recv() {
                res?;
                happy_to_drop.push(i);
            }
        }

        happy_to_drop.sort_unstable_by(|a, b| b.cmp(a));

        for i in happy_to_drop {
            app.mark_puts.remove(i);
        }

        if let Some(event) = event {
            if trying_to_quit || should_quit(event) {
                trying_to_quit = true;

                if app.mark_puts.is_empty() {
                    break;
                }
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
            AppState::Marking(_, _) => {
                marking::tick_app(&mut app, event).await?;
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
        AppState::Marking(_, _) => {
            marking::draw(frame, app, tickers);
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
