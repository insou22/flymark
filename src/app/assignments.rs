use std::{mem, collections::BTreeMap, sync::Arc};

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use crossterm::event::{Event, KeyCode};
use reqwest::Method;
use serde::Deserialize;
use tui::{Frame, backend::Backend};

use crate::{imark::{Globals, Authentication, Journal, Journals, JournalTag, JournalMeta}, ui::{AppPage, journals::JournalsUi, UiPage, assignments::AssignmentsUi}, util::task::{Task, TaskRunner}};

use super::journals::AppJournalList;

pub struct AppPostAuth<B> {
    globals: Globals,
    auth: Authentication,
    assignments: Vec<String>,
    current_assignment: usize,
    state: AppPostAuthState,
    ui: AssignmentsUi<B>,
}

pub enum AppPostAuthState {
    SelectingAssignment,
    LoadingJournals { task: Task<FetchJournalsOutput> },
}

pub struct FetchJournalsOutput {
    pub assignment: String,
    pub journals: Journals,
}

impl<B> AppPostAuth<B> {
    pub fn new(globals: Globals, auth: Authentication, assignments: Vec<String>) -> Self {
        Self {
            globals,
            auth,
            assignments,
            current_assignment: 0,
            state: AppPostAuthState::SelectingAssignment,
            ui: AssignmentsUi::new(),
        }
    }

    pub fn globals(&self) -> &Globals {
        &self.globals
    }

    pub fn auth(&self) -> &Authentication {
        &self.auth
    }

    pub fn assignments(&self) -> &Vec<String> {
        &self.assignments
    }

    pub fn current_assignment(&self) -> usize {
        self.current_assignment
    }

    pub fn state(&self) -> &AppPostAuthState {
        &self.state
    }
}

#[async_trait]
impl<B: Backend + Send + 'static> AppPage<B> for AppPostAuth<B> {
    async fn tick(&mut self, io: Option<Event>) -> Result<Option<Box<dyn AppPage<B>>>> {
        match &mut self.state {
            AppPostAuthState::SelectingAssignment => {}
            AppPostAuthState::LoadingJournals { task } => {
                if let Some(output) = task.poll()? {
                    let assignment = mem::take(&mut self.assignments[self.current_assignment]);

                    return Ok(Some(Box::new(
                        AppJournalList::new(
                            self.globals.clone(),
                            self.auth.clone(),
                            output.assignment,
                            output.journals
                        )
                    )));
                } else {
                    return Ok(None);
                }
            }
        }
        
        let event = match io {
            Some(event) => event,
            None => return Ok(None),
        };

        match event {
            Event::Key(key) => {
                match key.code {
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.current_assignment = (self.current_assignment + 1) % self.assignments.len();
                        
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.current_assignment = (self.current_assignment + self.assignments.len() - 1) % self.assignments.len();
                    }
                    KeyCode::Enter => {
                        let globals = self.globals.clone();
                        let auth = self.auth.clone();
                        let assignment = mem::take(&mut self.assignments[self.current_assignment]);

                        let task = Task::new(
                            FetchJournalsTask {
                                globals,
                                auth,
                                assignment
                            },
                            self.globals.panic_on_drop(),
                        );

                        self.state = AppPostAuthState::LoadingJournals { task };
                    }
                    _ => {}
                }
            }
            _ => {}
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

pub struct FetchJournalsTask {
    pub globals: Globals,
    pub auth: Authentication,
    pub assignment: String,
}

#[async_trait]
impl TaskRunner<FetchJournalsOutput> for FetchJournalsTask {
    async fn run(self) -> Result<FetchJournalsOutput> {
        #[derive(Deserialize, Debug)]
        pub struct JournalsJson {
            submissions: BTreeMap<String, Group>,
        }

        type Group = BTreeMap<String, SubmissionJson>;

        #[derive(Deserialize, Debug)]
        pub struct SubmissionJson {
            name: String,
            provisional_mark: Option<f64>,
            mark: Option<f64>,
            notes: Option<String>,
        }

        let imark_cgi_endpoint = self.globals.cgi_endpoint();
        let auth = self.auth;
        let assignment = self.assignment;

        let imark_full_endpoint = format!("{imark_cgi_endpoint}/api/v1/assignments/{assignment}/submissions/");

        let client = reqwest::Client::new();
        let resp: JournalsJson = client.request(Method::GET, imark_full_endpoint)
            .basic_auth(auth.username(), Some(auth.password()))
            .send()
            .await?
            .json()
            .await?;
        
        let mut journals = Journals::new(self.globals.clone());
        
        let mut flattened_journals = resp.submissions.into_iter()
            .map(|(group_id, group)| {
                group.into_iter()
                    .map(|(student_id, submission)| {
                        (
                            JournalTag::new(
                                assignment.to_string(),
                                group_id.to_string(),
                                student_id,
                            ),
                            JournalMeta::new(
                                submission.name,
                                submission.provisional_mark,
                                submission.mark,
                                submission.notes,
                            )
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .flatten()
            .collect::<Vec<_>>();

        if flattened_journals.len() == 0 {
            return anyhow::Result::Err(anyhow!("No journals found for selected assignment"));
        }

        for (tag, meta) in flattened_journals {
            journals.insert(tag, meta);
        }

        anyhow::Ok(
            FetchJournalsOutput {
                assignment,
                journals,
            }
        )
    }
}
