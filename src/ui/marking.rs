use std::{collections::BTreeMap, io::{Write, Read, Seek}, os::unix::prelude::AsRawFd, process, path::Path};

use anyhow::{anyhow, Result, Context};
use crossterm::event::{Event, KeyCode};
use memfile::{MemFile, CreateOptions, Seal};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use tmux_interface::{SplitWindow, RespawnPane};
use tokio::{sync::oneshot, fs::{symlink, remove_file}};
use tui::{backend::Backend, Frame, widgets::{Borders, Block, Paragraph, ListState, ListItem, List}, layout::{Constraint, Direction, Layout}, style::{Style, Color}, text::Span};

use crate::choices::Choice;

use super::{App, AppState, UiTickers, BasicAuth, journals::{Journal, JournalDetails, JournalDetailsData}};

pub enum MarkingState {
    ReadyToLoad    { journal_index: usize },
    LoadingContent { journal_index: usize, channel: oneshot::Receiver<MarkingTaskOutput> },
    LoadingFromPreload { journal_index: usize },
    Loaded  { journal_index: usize },
    Marking { journal_index: usize, list_state: ListState, choices: Vec<(Choice, bool)> },
}

#[derive(Debug)]
pub enum MarkingTaskOutput {
    Success,
    AlreadyInProgress,
    Failure { failure: anyhow::Error },
}

pub async fn tick_app(app: &mut App<'_>, io_event: Option<Event>) -> Result<()> {
    match &mut app.state {
        AppState::Marking(journals, mark_state @ MarkingState::ReadyToLoad { .. }) => {
            // this should be mark_state @ MarkingState::ReadyToLoad { journal_index }
            // fucking borrowck
            let journal_index = match mark_state {
                MarkingState::ReadyToLoad { journal_index } => journal_index,
                _ => unreachable!(),
            };

            let (sender, receiver) = tokio::sync::oneshot::channel();

            tokio::spawn(populate_journal(
                Some(sender),
                app.params.endpoint.to_string(),
                app.auth.as_ref().unwrap().clone(),
                journals[*journal_index].clone(),
            ));

            *mark_state = MarkingState::LoadingContent {
                journal_index: *journal_index,
                channel: receiver,
            }
        }
        AppState::Marking(_journals, mark_state @ MarkingState::LoadingContent { .. }) => {
            // this should be mark_state @ MarkingState::LoadingContent { journal_index, channel }
            // fucking borrowck
            let (journal_index, channel) = match mark_state {
                MarkingState::LoadingContent { journal_index, channel } => (journal_index, channel),
                _ => unreachable!(),
            };

            if let Ok(output) = channel.try_recv() {
                match output {
                    MarkingTaskOutput::Success => {
                        *mark_state = MarkingState::Loaded { journal_index: *journal_index };
                    },
                    MarkingTaskOutput::AlreadyInProgress => {
                        *mark_state = MarkingState::LoadingFromPreload { journal_index: *journal_index };
                    },
                    MarkingTaskOutput::Failure { failure } => {
                        anyhow::Result::Err(failure)
                            .context("Failed to populate a journal; did imark die?")?;
                    },
                }
            }
        }
        AppState::Marking(journals, mark_state @ MarkingState::LoadingFromPreload { .. }) => {
            // this should be mark_state @ MarkingState::LoadingFromPreload { journal_index }
            // fucking borrowck
            let journal_index = match mark_state {
                MarkingState::LoadingFromPreload { journal_index } => (journal_index),
                _ => unreachable!(),
            };

            let journal = &journals[*journal_index];

            {
                let details = journal.details.lock();

                let loaded = match *details {
                    JournalDetails::Unloaded  => {
                        return anyhow::Result::Err(anyhow!("Journal just... never started to load?"));
                    }
                    JournalDetails::Loading   => {
                        false
                    }
                    JournalDetails::Loaded(_) => {
                        true
                    }
                };

                if loaded {
                    *mark_state = MarkingState::Loaded { journal_index: *journal_index };
                }
            }
        }
        _ => {}
    }

    match &mut app.state {
        AppState::Marking(journals, mark_state @ MarkingState::Loaded { .. }) => {
            // this should be mark_state @ MarkingState::Loaded { journal_index }
            // fucking borrowck
            let journal_index = match mark_state {
                MarkingState::Loaded { journal_index } => *journal_index,
                _ => unreachable!(),
            };

            let journal = &journals[journal_index];
            
            let files = {
                let details = journal.details.lock();
                let loaded = match &*details {
                    JournalDetails::Loaded(data) => data,
                    _ => unreachable!(),
                };

                (
                    loaded.submission_files.iter()
                        .map(|(name, file)| (name.to_string(), file.as_raw_fd()))
                        .collect::<Vec<_>>(),

                    loaded.marking_files.iter()
                        .map(|(name, file)| (name.to_string(), file.as_raw_fd()))
                        .collect::<Vec<_>>(),
                )
            };

            let pid = process::id();
            let mut shell_command = String::from(app.params.pager_command);
            for (name, fd) in files.0.iter().chain(files.1.iter()) {
                if Path::exists(Path::new(name)) {
                    remove_file(name).await?;
                }
                symlink(format!("/proc/{pid}/fd/{fd}"), name).await?;
                shell_command += " ";
                shell_command += name;
            }

            match app.side_pane_id.as_ref() {
                Some(id) => {
                    RespawnPane::new()
                        .kill()
                        .target_pane(id)
                        .shell_command(shell_command)
                        .output()?;
                }
                None => {
                    let pane_id = String::from_utf8(
                        SplitWindow::new()
                            .print()
                            .horizontal()
                            .detached()
                            .shell_command(shell_command)
                            .output()?
                            .stdout()
                    )?.trim().to_string();
    
                    app.side_pane_id = Some(pane_id);
                }
            }

            let real_first_index = app.params.choices.choices.iter()
                .enumerate()
                .find(|(_, choice)| !matches!(choice, Choice::Comment(_)))
                .unwrap()
                .0;

            let mut list_state = ListState::default();
            list_state.select(Some(real_first_index));

            *mark_state = MarkingState::Marking {
                journal_index: journal_index,
                list_state,
                choices: app.params.choices.choices.iter()
                    .cloned()
                    .map(|choice| (choice, false))
                    .collect(),
            };

            for i in 1..=3 {
                if let Some(next_journal) = journals.get(journal_index + i) {
                    // preload the next journals
                    if matches!(*next_journal.details.lock(), JournalDetails::Unloaded) {
                        tokio::spawn(populate_journal(
                            None,
                            app.params.endpoint.to_string(),
                            app.auth.as_ref().unwrap().clone(),
                            next_journal.clone(),
                        ));
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
        AppState::Marking(journals, mark_state @ MarkingState::Marking { .. }) => {
            // fucking borrowck
            let (journal_index, list_state, choices) = match mark_state {
                MarkingState::Marking { journal_index, list_state, choices } => (*journal_index, list_state, choices),
                _ => unreachable!(),
            };

            match event {
                Event::Key(key) => {
                    let real_first_index = choices.iter()
                        .enumerate()
                        .find(|(_, (choice, _))| !matches!(choice, Choice::Comment(_)))
                        .unwrap()
                        .0;

                    let real_last_index = choices.iter()
                        .enumerate()
                        .filter(|(_, (choice, _))| !matches!(choice, Choice::Comment(_)))
                        .last()
                        .unwrap()
                        .0;

                    match key.code {
                        KeyCode::Down  => {
                            let current = list_state.selected().unwrap();

                            list_state.select(Some(
                                if current == real_last_index {
                                    real_first_index
                                } else {
                                    let mut new = current + 1;

                                    while matches!(choices[new].0, Choice::Comment(_)) {
                                        new += 1;
                                    }

                                    new
                                }
                            ));
                            
                        }
                        KeyCode::Up    => {
                            let current = list_state.selected().unwrap();
                            
                            list_state.select(Some(
                                if current == real_first_index {
                                    real_last_index
                                } else {
                                    let mut new = current - 1;

                                    while matches!(choices[new].0, Choice::Comment(_)) {
                                        new -= 1;
                                    }

                                    new
                                }
                            ));
                        }
                        KeyCode::Char(' ') | KeyCode::Right => {
                            let current = list_state.selected().unwrap();
                            choices[current].1 = !choices[current].1;

                            if matches!(choices[current].0, Choice::Set(_, _)) {
                                for (idx, choice) in choices.iter_mut().enumerate() {
                                    if idx != current {
                                        choice.1 = false;
                                    }
                                }
                            } else {
                                for choice in choices.iter_mut() {
                                    if matches!(choice.0, Choice::Set(_, _)) {
                                        choice.1 = false;
                                    }
                                }
                            }
                        }
                        KeyCode::Enter => {
                            let mut mark = 0;
                            let mut comments = vec![];

                            for choice in choices.iter()
                                .filter(|(_, selected)| *selected)
                                .map(|(choice, _)| choice)
                            {
                                match choice {
                                    Choice::Plus(n, comment) => {
                                        mark += n;
                                        comments.push(format!("+{n} {comment}"));
                                    }
                                    Choice::Minus(n, comment) => {
                                        mark -= n;
                                        comments.push(format!("-{n} {comment}"));

                                    }
                                    Choice::Set(n, comment) => {
                                        mark = *n;
                                        comments.push(format!("{n} {comment}"));
                                    }
                                    Choice::Comment(_)  => {}
                                }
                            }

                            let journal = &journals[journal_index];
                            let (journal_mark_name, mut journal_mark_text) = {
                                match &mut *journal.details.lock() {
                                    JournalDetails::Loaded(journal) => {
                                        let (name, file) = journal.marking_files.iter_mut()
                                            .find(|(name, _)| name == "performance")
                                            .expect("performance mark must exist");

                                        let mut text = String::new();
                                        file.seek(std::io::SeekFrom::Start(0))?;
                                        file.read_to_string(&mut text)?;

                                        (name.to_string(), text)
                                    }
                                    _ => unreachable!(),
                                }
                            };

                            let mut body = MarkPut {
                                marks: BTreeMap::new(),
                                comments: BTreeMap::new(),
                            };

                            let at = chrono::Local::now().format("%F %T%.6f").to_string();
                            let by = app.auth.as_ref().unwrap().username.to_string();

                            journal_mark_text += &format!("\nmarked with flymark by {by} at {at}\n\n");

                            for comment in comments {
                                journal_mark_text += &comment;
                                journal_mark_text += "\n";
                            }

                            body.marks.insert(
                                "1".to_string(),
                                Mark {
                                    at,
                                    by,
                                    is_final: true,
                                    mark: mark as f64,
                                    name: journal_mark_name,
                                    text: journal_mark_text,
                                }
                            );

                            let imark = app.params.endpoint;
                            let assign = &journal.assignment;
                            let group  = &journal.group_id;
                            let stuid  = &journal.student_id;
                            
                            let endpoint = format!("{imark}/api/v1/assignments/{assign}/submissions/{group}/{stuid}/");

                            let (sender, receiver) = oneshot::channel::<Result<()>>();

                            app.mark_puts.push(receiver);

                            tokio::spawn(
                                submit_journal(
                                    sender,
                                    endpoint,
                                    app.auth.as_ref().unwrap().clone(),
                                    body,
                                )
                            );

                            let journal_index = journal_index + 1;

                            *mark_state = MarkingState::ReadyToLoad { journal_index };
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
    
    Ok(())
}

#[derive(Deserialize)]
pub struct SubmissionJson {
    files: BTreeMap<String, FileJson>,
    marks: BTreeMap<String, MarkJson>,
}

#[derive(Deserialize)]
pub struct FileJson {
    name: String,
    contents: String,
}

#[derive(Deserialize)]
pub struct MarkJson {
    name: String,
    text: String,
}

async fn populate_journal(
    sender: Option<oneshot::Sender<MarkingTaskOutput>>,
    imark_endpoint: String,
    auth: BasicAuth,
    journal: Journal,
) {
    {
        let mut details = journal.details.lock();

        match *details {
            JournalDetails::Loaded(_) => {
                if let Some(sender) = sender {
                    // take credit for their work
                    sender.send(MarkingTaskOutput::Success).unwrap();
                }
                return;
            }
            JournalDetails::Loading => {
                if let Some(sender) = sender {
                    sender.send(MarkingTaskOutput::AlreadyInProgress).unwrap();
                }
                return;
            }
            JournalDetails::Unloaded => {}
        }

        *details = JournalDetails::Loading;
    }

    let body = || async {
        let assignment = journal.assignment.as_str();
        let group_id   = journal.group_id.as_str();
        let student_id = journal.student_id.as_str();

        let client = reqwest::Client::new();
        let resp: SubmissionJson = client.request(
            Method::GET,
            format!("{imark_endpoint}/api/v1/assignments/{assignment}/submissions/{group_id}/{student_id}/")
        )
            .basic_auth(auth.username(), Some(auth.password()))
            .send()
            .await?
            .json()
            .await?;

        let mut submission_files = vec![];
        let mut marking_files    = vec![];

        for (_imark_name, file) in resp.files {
            let mut mem_file = MemFile::create("memfile", CreateOptions::new().allow_sealing(true))?;
            mem_file.write_all(file.contents.as_bytes())?;
            mem_file.add_seals(Seal::Write | Seal::Shrink | Seal::Grow)?;

            submission_files.push((file.name, mem_file));
        }

        for (_imark_name, file) in resp.marks {
            let mut mem_file = MemFile::create("memfile", CreateOptions::new().allow_sealing(true))?;
            mem_file.write_all(file.text.as_bytes())?;
            mem_file.add_seals(Seal::Write | Seal::Shrink | Seal::Grow)?;

            marking_files.push((file.name, mem_file));
        }

        let details_data = JournalDetailsData {
            submission_files,
            marking_files,
        };

        {
            let mut lock = journal.details.lock();
            *lock = JournalDetails::Loaded(details_data);
        }

        anyhow::Ok(())
    };

    let result = body().await;

    if let Some(sender) = sender {
        sender.send(
            match result {
                Ok(_)    => MarkingTaskOutput::Success { },
                Err(err) => MarkingTaskOutput::Failure { failure: err },
            }
        ).expect("receiver should not drop before sending");
    } else {
        match result {
            Ok(_) => {}
            Err(_) => {
                {
                    // let someone else run into it
                    let mut details = journal.details.lock();
                    *details = JournalDetails::Unloaded;
                }
            }
        }
    }
}


#[derive(Serialize)]
struct MarkPut {
    marks: BTreeMap<String, Mark>,
    comments: BTreeMap<String, ()>,
}

#[derive(Serialize)]
struct Mark {
    at: String,
    by: String,
    name: String,
    is_final: bool,
    mark: f64,
    text: String,
}

async fn submit_journal(
    sender: oneshot::Sender<Result<()>>,
    full_endpoint: String,
    auth: BasicAuth,
    body: MarkPut,
) {
    let main = || async {
        dbg!(reqwest::Client::new()
            .put(full_endpoint)
            .basic_auth(auth.username(), Some(auth.password()))
            .json(&body)
            .send()
            .await?
            .text()
            .await?
        );

        anyhow::Ok(())
    };
    
    let result = main().await;
    sender.send(result).expect("receiver should not drop before sending");
}


pub fn draw<B: Backend>(frame: &mut Frame<B>, app: &mut App, tickers: &mut UiTickers) {
    match &mut app.state {
        AppState::Marking(_journals, MarkingState::Marking { journal_index: _, list_state, choices }) => {
            let size = frame.size();

            const INFO_HEIGHT: u16 = 3;
            const MARGIN: u16 = 2;

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .vertical_margin(MARGIN)
                .constraints(
                    [
                        Constraint::Length(INFO_HEIGHT),
                        Constraint::Length(MARGIN),
                        Constraint::Length(size.height.saturating_sub(INFO_HEIGHT)),
                    ]
                )
                .split(size);

            let info =
                Paragraph::new(
                    "Press <space> to toggle a choice\n\
                     Press <up>/<down> to select a choice\n\
                     Press <enter> to submit and move to next journal"
                ).block(Block::default()
                        .borders(Borders::NONE)
                );

            frame.render_widget(info, chunks[0]);
            
            let list_items = choices.iter()
                .map(|(choice, selected)| {
                    ListItem::new(Span::styled(
                        match choice {
                            Choice::Plus (n, text) => {
                                format!("+{n} {text}")
                            }
                            Choice::Minus(n, text) => {
                                format!("-{n} {text}")
                            }
                            Choice::Set  (n, text) => {
                                format!("{n} {text}")
                            }
                            Choice::Comment(text)  => {
                                text.to_string()
                            }
                        },
                        if *selected {
                            Style::default()
                                .bg(Color::White)
                                .fg(Color::Black)
                        } else {
                            Style::default()
                        }
                    ))
                })
                .collect::<Vec<_>>();

            let list = List::new(list_items)
                .block(
                    Block::default()
                        .title("Mark")
                        .borders(Borders::ALL)
                )
                // .style(Style::default().fg(Color::White))
                // .highlight_style(Style::default().add_modifier(Modifier::ITALIC))
                .highlight_symbol(">> ");

            frame.render_stateful_widget(list, chunks[2], list_state);
        }
        AppState::Marking(_journals, _) => {
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
        _ => unreachable!()
    }
}
