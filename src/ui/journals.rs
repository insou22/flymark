use std::{mem, collections::BTreeMap, sync::{Arc}, cmp::Ordering};

use anyhow::{anyhow, Result, Context};
use crossterm::event::{Event, KeyCode};
use memfile::MemFile;
use parking_lot::Mutex;
use reqwest::Method;
use serde::Deserialize;
use tokio::{sync::oneshot, task};
use tui::{Frame, backend::Backend, layout::{Layout, Direction, Constraint}, widgets::{Paragraph, Block, Borders, List, ListItem, ListState}, style::{Style, Color, Modifier}};

use super::{App, UiTickers, AppState, BasicAuth, marking::MarkingState};

pub enum JournalsState {
    Waiting         { assignment: String },
    Loading         { assignment: String, channel: oneshot::Receiver<JournalsTaskOutput> },
    ChoosingJournal { journals: Vec<Journal>, list_state: ListState },
}

impl JournalsState {
    pub fn new(assignment: String) -> Self {
        Self::Waiting { assignment }
    }
}

#[derive(Debug, Clone)]
pub struct Journal {
    pub assignment: String,
    pub group_id: String,
    pub student_id: String,
    pub name: Option<String>,
    pub mark: Option<f64>,
    pub provisional_mark: Option<f64>,
    pub details: Arc<Mutex<JournalDetails>>,
}

#[derive(Debug)]
pub enum JournalDetails {
    Unloaded,
    Loading,
    Loaded(JournalDetailsData),
}

#[derive(Debug)]
pub struct JournalDetailsData {
    pub submission_files: Vec<(String, MemFile)>,
    pub marking_files:    Vec<(String, MemFile)>,
}

#[derive(Debug)]
pub enum JournalsTaskOutput {
    Success { journals: JournalsJson },
    Failure { failure: anyhow::Error },
}

pub fn tick_app(app: &mut App<'_>, io_event: Option<Event>) -> Result<()> {
    match &mut app.state {
        AppState::Journals(JournalsState::Waiting { assignment }) => {
            let (sender, receiver) = oneshot::channel();

            task::spawn(fetch_journals(
                sender,
                app.params.endpoint.to_string(),
                app.auth.as_ref().expect("Must be authenticated by now").clone(),
                assignment.to_string()
            ));

            app.state = AppState::Journals(JournalsState::Loading {
                assignment: mem::take(assignment),
                channel: receiver
            });
        }
        AppState::Journals(JournalsState::Loading { assignment, channel }) => {
            if let Ok(response) = channel.try_recv() {
                match response {
                    JournalsTaskOutput::Success { journals } => {
                        let mut flattened_journals = journals.submissions.into_iter()
                            .map(|(group_id, group)| {
                                group.into_iter()
                                    .map(|(student_id, submission)| {
                                        Journal {
                                            assignment: assignment.to_string(),
                                            group_id:   group_id.to_string(),
                                            student_id,
                                            name: submission.name,
                                            mark: submission.mark,
                                            provisional_mark: submission.provisional_mark,
                                            details: Arc::new(Mutex::new(JournalDetails::Unloaded)),
                                        }
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .flatten()
                            .collect::<Vec<_>>();

                        if flattened_journals.len() == 0 {
                            return anyhow::Result::Err(anyhow!("No journals found for selected assignment"));
                        }

                        flattened_journals.sort_by(|a, b| {
                            match (a.mark, b.mark) {
                                (Some(a), Some(b)) => b.partial_cmp(&a).unwrap_or(Ordering::Equal),
                                (Some(_), None) => Ordering::Greater,
                                (None, Some(_)) => Ordering::Less,
                                (None, None) => {
                                    match (a.provisional_mark, b.provisional_mark) {
                                        (Some(a), Some(b)) => b.partial_cmp(&a).unwrap_or(Ordering::Equal),
                                        (Some(_), None) => Ordering::Greater,
                                        (None, Some(_)) => Ordering::Less,
                                        (None, None) => a.student_id.cmp(&b.student_id),
                                    }
                                }
                            }
                        });

                        let mut list_state = ListState::default();
                        list_state.select(Some(0));

                        app.state = AppState::Journals(JournalsState::ChoosingJournal {
                            journals: flattened_journals,
                            list_state,
                        });
                    }
                    JournalsTaskOutput::Failure { failure } => {
                        anyhow::Result::Err(failure)
                            .context("Failed to load journals; did imark die?")?;
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
        AppState::Journals(JournalsState::ChoosingJournal { journals, list_state }) => {
            match event {
                Event::Key(key) => {
                    match key.code {
                        KeyCode::Down  => {
                            let current = list_state.selected().unwrap();
                            
                            list_state.select(Some(
                                if current == journals.len() - 1 {
                                    0
                                } else {
                                    current + 1
                                }
                            ));
                            
                        }
                        KeyCode::Up    => {
                            let current = list_state.selected().unwrap();
                            
                            list_state.select(Some(
                                if current == 0 {
                                    journals.len() - 1
                                } else {
                                    current - 1
                                }
                            ));
                        }
                        KeyCode::Enter => {
                            let current = list_state.selected().unwrap();
                            app.state = AppState::Marking(
                                mem::take(journals),
                                MarkingState::ReadyToLoad { journal_index: current }
                            );
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        AppState::Journals(_) => {}
        _ => unreachable!(),
    }
    
    Ok(())
}

pub fn draw<B: Backend>(frame: &mut Frame<B>, app: &mut App, tickers: &mut UiTickers) {
    match &mut app.state {
        AppState::Journals(JournalsState::ChoosingJournal { journals, list_state }) => {
            let size = frame.size();
            
            let list_items = journals.iter()
                .map(|journal| {
                    ListItem::new(
                        format!(
                            "{} | {:6} | {}",
                            journal.student_id.as_str(),
                            journal.mark.map(|m| format!("{:>5.02}", m))
                                .or(journal.provisional_mark.map(|m| format!("{:>5.02}?", m)))
                                .unwrap_or_else(|| "".to_string()),
                            journal.name.as_ref().map(|s| s.as_str()).unwrap_or_else(|| ""),
                        )
                    )
                })
                .collect::<Vec<_>>();

            let list = List::new(list_items)
                .block(
                    Block::default()
                        .title("Choose a journal")
                        .borders(Borders::ALL)
                )
                .style(Style::default().fg(Color::White))
                .highlight_style(Style::default().add_modifier(Modifier::ITALIC))
                .highlight_symbol(">> ");
            
            frame.render_stateful_widget(list, size, list_state);
        }
        AppState::Journals(_) => {
            let size = frame.size();

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
        _ => unreachable!(),
    }
}

#[derive(Deserialize, Debug)]
pub struct JournalsJson {
    submissions: BTreeMap<String, Group>,
}

type Group = BTreeMap<String, SubmissionJson>;

#[derive(Deserialize, Debug)]
pub struct SubmissionJson {
    mark: Option<f64>,
    name: Option<String>,
    provisional_mark: Option<f64>,
}

async fn fetch_journals(
    sender: oneshot::Sender<JournalsTaskOutput>,
    imark_endpoint: String,
    auth: BasicAuth,
    assignment: String,
) {
    let body = || async {
        let client = reqwest::Client::new();
        let resp: JournalsJson = client.request(Method::GET, format!("{imark_endpoint}/api/v1/assignments/{assignment}/submissions/"))
            .basic_auth(auth.username(), Some(auth.password()))
            .send()
            .await?
            .json()
            .await?;

        anyhow::Ok(resp)
    };

    sender.send(
        match body().await {
            Ok(body) => JournalsTaskOutput::Success { journals: body },
            Err(err) => JournalsTaskOutput::Failure { failure: err },
        }
    ).expect("receiver should not drop before sending");
}
