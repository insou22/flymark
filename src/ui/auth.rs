use std::mem;

use anyhow::{Result, Context};
use crossterm::event::Event;
use reqwest::Method;
use tokio::{task, sync::oneshot};
use tui::{backend::Backend, Frame, widgets::{Borders, Block, Paragraph}, layout::{Constraint, Direction, Layout}, style::{Style, Color}};
use tui_input::{Input, backend::crossterm as tui_input_crossterm, InputResponse};

use super::{App, AppState, UiTickers, assignments::AssignmentsState, BasicAuth};

pub enum AuthenticatingState {
    EnteringZid { zid_input: Input, },
    EnteringPassword { zid: String, password_input: Input, },
    AuthenticationReady { zid: String, password: String },
    AuthenticationWaiting { output: oneshot::Receiver<AuthTaskOutput> },
    // AuthenticationFailed,
}

#[derive(Debug)]
pub enum AuthTaskOutput {
    Success { assignments: Vec<String>, zid: String, password: String, },
    Failure { failure: anyhow::Error },
}

pub fn tick_app(app: &mut App<'_>, io_event: Option<Event>) -> Result<()> {
    match &mut app.state {
        AppState::Authenticating(AuthenticatingState::AuthenticationWaiting { output }) => {
            if let Ok(response) = output.try_recv() {
                match response {
                    AuthTaskOutput::Success { assignments, zid, password } => {
                        app.auth  = Some(BasicAuth::new(zid, password));
                        app.state = AppState::Choosing(AssignmentsState::new(assignments));
                    }
                    AuthTaskOutput::Failure { failure } => {
                        anyhow::Result::Err(failure)
                            .context("Failed to authenticate -- check your zID/zPass, imark endpoint, internet connection")?;
                    }
                }
            }
        }
        _ => {}
    }

    let event = match io_event {
        Some(event) => event,
        None => return Ok(()),
    };

    match &mut app.state {
        AppState::Authenticating(state) => {
            match state {
                AuthenticatingState::EnteringZid { zid_input: input }
                | AuthenticatingState::EnteringPassword { zid: _, password_input: input } => {
                    let response = tui_input_crossterm::to_input_request(event)
                        .and_then(|req| input.handle(req));
                    
                    match response {
                        Some(InputResponse::StateChanged(_)) => {}
                        Some(InputResponse::Submitted) => {
                            match state {
                                AuthenticatingState::EnteringZid { zid_input } => {
                                    let zid = zid_input.value().to_string();
                                    *state = AuthenticatingState::EnteringPassword { zid, password_input: Input::default() };
                                }
                                AuthenticatingState::EnteringPassword { zid, password_input } => {
                                    let zid = mem::take(zid);
                                    let password = password_input.value().to_string();
                                    *state = AuthenticatingState::AuthenticationReady { zid, password };
                                }
                                _ => unreachable!("cases covered above"),
                            }
                        }
                        Some(InputResponse::Escaped) => {}
                        None => {}
                    }
                }
                AuthenticatingState::AuthenticationReady   { .. } => {}
                AuthenticatingState::AuthenticationWaiting { .. } => {}
                // AuthenticatingState::AuthenticationFailed => {}
            }

            match state {
                AuthenticatingState::AuthenticationReady { zid, password } => {
                    let (sender, receiver) = oneshot::channel();

                    let zid = mem::take(zid);
                    let password = mem::take(password);

                    *state = AuthenticatingState::AuthenticationWaiting {
                        output: receiver,
                    };

                    task::spawn(do_auth(sender, app.params.endpoint.to_string(), zid, password));
                }
                _ => {}
            }
        }
        _ => {}
    }
    
    Ok(())
}

async fn do_auth(sender: oneshot::Sender<AuthTaskOutput>, imark_endpoint: String, zid: String, password: String) {
    let body = || async {
        let client = reqwest::Client::new();
        let resp: Vec<String> = client.request(Method::GET, format!("{imark_endpoint}/api/v1/assignments/"))
            .basic_auth(&zid, Some(&password))
            .send()
            .await?
            .json()
            .await?;

        anyhow::Ok(resp)
    };

    sender.send(
        match body().await {
            Ok(body) => AuthTaskOutput::Success { assignments: body, zid, password },
            Err(err) => AuthTaskOutput::Failure { failure: err },
        }
    ).expect("receiver should not drop before sending");
    
}

pub fn draw<B: Backend>(frame: &mut Frame<B>, app: &mut App, tickers: &mut UiTickers) {
    match &app.state {
        AppState::Authenticating(state) => {
            let size = frame.size();

            const INPUT_HEIGHT: u16 = 3;
            const INPUT_WIDTH:  u16 = 30;

            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints(
                    [
                        Constraint::Length(size.width.saturating_sub(INPUT_WIDTH) / 2),
                        Constraint::Length(INPUT_WIDTH + size.width % 2),
                        Constraint::Length(size.width.saturating_sub(INPUT_WIDTH) / 2),
                    ]
                )
                .split(size);

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints(
                    [
                        Constraint::Length(size.height.saturating_sub(INPUT_HEIGHT * 2) / 2),
                        Constraint::Length(INPUT_HEIGHT),
                        Constraint::Length(INPUT_HEIGHT),
                        Constraint::Length(size.height.saturating_sub(INPUT_HEIGHT * 2) / 2),
                    ]
                )
                .split(chunks[1]);

            match state {
                AuthenticatingState::EnteringZid { zid_input } => {
                    let zid_paragraph = Paragraph::new(zid_input.value())
                        .block(
                            Block::default()
                                .borders(Borders::ALL)
                                .title("zID")
                                .border_style(Style::default().fg(Color::LightGreen))
                            );

                    frame.render_widget(zid_paragraph, chunks[1]);
                    
                    let password_paragraph = Paragraph::new("")
                        .block(
                            Block::default()
                                .borders(Borders::ALL)
                                .title("password")
                        );

                    frame.render_widget(password_paragraph, chunks[2]);

                    frame.set_cursor(
                        chunks[1].x + zid_input.cursor() as u16 + 1,
                        chunks[1].y + 1,
                    )
                }
                AuthenticatingState::EnteringPassword { zid, password_input } => {
                    let zid_paragraph = Paragraph::new(zid.as_str())
                        .block(
                            Block::default()
                            .borders(Borders::ALL)
                            .title("zID")
                        );

                    frame.render_widget(zid_paragraph, chunks[1]);

                    let password_paragraph = Paragraph::new(
                            String::from("*").repeat(password_input.value().len())
                        )
                        .block(
                            Block::default()
                            .borders(Borders::ALL)
                            .title("password")
                            .border_style(Style::default().fg(Color::LightGreen))
                        );

                    frame.render_widget(password_paragraph, chunks[2]);

                    frame.set_cursor(
                        chunks[2].x + password_input.cursor() as u16 + 1,
                        chunks[2].y + 1,
                    )

                }
                AuthenticatingState::AuthenticationReady { .. }
                | AuthenticatingState::AuthenticationWaiting { .. } => {
                    const INPUT_HEIGHT: u16 = 1;
                    const INPUT_WIDTH:  u16 = 10;

                    let chunks = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints(
                            [
                                Constraint::Length(size.width.saturating_sub(INPUT_WIDTH) / 2),
                                Constraint::Length(INPUT_WIDTH + size.width % 2),
                                Constraint::Length(size.width.saturating_sub(INPUT_WIDTH) / 2),
                            ]
                        )
                        .split(size);

                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints(
                            [
                                Constraint::Length(size.height.saturating_sub(INPUT_HEIGHT) / 2),
                                Constraint::Length(INPUT_HEIGHT + size.height % 2),
                                Constraint::Length(size.height.saturating_sub(INPUT_HEIGHT) / 2),
                            ]
                        )
                        .split(chunks[1]);

                    let loading = Paragraph::new(String::from("Loading") + &".".repeat((tickers.auth_loading % 81) / 27 + 1))
                        .block(
                            Block::default()
                            .borders(Borders::NONE)
                        );
                    
                    frame.render_widget(loading, chunks[1]);
                    tickers.auth_loading = tickers.auth_loading.wrapping_add(1);
                }
                // AuthenticatingState::AuthenticationFailed => {}
            }
        }
        _ => {}
    }
}
