use std::mem;

use anyhow::Result;
use async_trait::async_trait;
use crossterm::event::Event;
use reqwest::Method;
use tokio::sync::oneshot;
use tui::{backend::Backend, Frame};
use tui_input::{Input, InputResponse, backend::crossterm as tui_input_crossterm};
use crate::{ui::{AppPage, UiPage, auth::AuthUi}, imark::{Globals, Authentication}, util::task::{Task, TaskRunner, self}};

use super::assignments::AppPostAuth;

pub struct AppPreAuth<B> {
    globals: Globals,
    state: AppPreAuthState,
    ui: AuthUi<B>,
}

pub enum AppPreAuthState {
    EnteringZid { zid_input: Input },
    EnteringPassword { zid: String, password_input: Input },
    Authenticating { zid: String, password: String, task: Task<AuthTaskOutput> },
}

pub struct AuthTaskOutput {
    assignments: Vec<String>,
}

impl<B> AppPreAuth<B> {
    pub fn new(globals: Globals) -> Self {
        Self {
            globals,
            state: AppPreAuthState::EnteringZid { zid_input: Input::default() },
            ui: AuthUi::new()
        }
    }

    pub fn globals(&self) -> &Globals {
        &self.globals
    }
    
    pub fn state(&self) -> &AppPreAuthState {
        &self.state
    }
}

#[async_trait]
impl<B: Backend + Send + 'static> AppPage<B> for AppPreAuth<B> {
    async fn tick(&mut self, io: Option<Event>) -> Result<Option<Box<dyn AppPage<B>>>> {
        match &mut self.state {
            AppPreAuthState::EnteringZid { zid_input } => {
                if let Some(event) = io {
                    let submitted = process_input(event, zid_input);

                    if submitted {
                        self.state = AppPreAuthState::EnteringPassword {
                            zid: zid_input.value().to_string(),
                            password_input: Input::default(),
                        };
                    }
                }
            }
            AppPreAuthState::EnteringPassword { zid, password_input } => {
                if let Some(event) = io {
                    let submitted = process_input(event, password_input);
                    if submitted {
                        let password = password_input.value().to_string();

                        let zid = mem::take(zid);

                        self.state = AppPreAuthState::Authenticating {
                            zid: zid.to_string(),
                            password: password.to_string(),
                            task: Task::new(AuthenticateTask::new(
                                self.globals.cgi_endpoint().to_string(),
                                zid,
                                password,
                            )),
                        };
                    }
                }
            }
            AppPreAuthState::Authenticating { zid, password, task } => {
                if let Some(output) = task.poll()? {
                    return Ok(Some(Box::new(
                        AppPostAuth::new(
                            self.globals.clone(),
                            Authentication::new(mem::take(zid), mem::take(password)),
                            output.assignments,
                        )
                    )));
                }
            }
        }

        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame<B>) {
        self.ui.draw(self, frame);
        self.ui.update();
    }

    async fn quit(&mut self) -> Result<()> {
        Ok(())
    }
}

struct AuthenticateTask {
    imark_cgi_endpoint: String,
    zid: String,
    password: String,
}

impl AuthenticateTask {
    pub fn new(imark_cgi_endpoint: String, zid: String, password: String) -> Self {
        Self {
            imark_cgi_endpoint,
            zid,
            password,
        }
    }
}

#[async_trait]
impl TaskRunner<AuthTaskOutput> for AuthenticateTask {
    async fn run(self) -> Result<AuthTaskOutput> {
        let imark_cgi_endpoint = self.imark_cgi_endpoint;
        let zid = self.zid;
        let password = self.password;

        let client = reqwest::Client::new();
        let resp: Vec<String> = client.request(Method::GET, format!("{imark_cgi_endpoint}/api/v1/assignments/"))
            .basic_auth(&zid, Some(&password))
            .send()
            .await?
            .json()
            .await?;

        anyhow::Ok(AuthTaskOutput { assignments: resp })
    }
}

type Submitted = bool;
fn process_input(event: Event, input: &mut Input) -> Submitted {
    let response = tui_input_crossterm::to_input_request(event)
        .and_then(|req| input.handle(req));

    match response {
        Some(InputResponse::Submitted) => true,
        Some(InputResponse::StateChanged(_))
        | Some(InputResponse::Escaped)
        | None => false,
    }
}
