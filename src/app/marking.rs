use std::{process, path::Path, os::unix::prelude::AsRawFd, mem};

use anyhow::Result;
use async_trait::async_trait;
use crossterm::event::{Event, KeyCode, KeyModifiers};
use tmux_interface::{RespawnPane, SplitWindow};
use tokio::fs::{remove_file, symlink, read_link};
use tui::{backend::Backend, Frame};

use crate::{imark::{Globals, Authentication, Journals, JournalTag, BidirectionalIterator}, choice::{ChoiceSelections, Choice}, ui::{marking::MarkingUi, AppPage, UiPage}, util::{task::Task, tmux::TmuxPane, HOTKEYS}};

use super::{assignments::{FetchJournalsOutput, FetchJournalsTask}, journals::AppJournalList};

pub struct AppMarking<B> {
    globals: Globals,
    auth: Authentication,
    assignment: String,
    journals: Journals,
    live_journal_tag: JournalTag,
    opened: Opened,
    tmux_side_pane: Option<TmuxPane>,
    state: AppMarkingState,
    ui: MarkingUi<B>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Opened {
    Automatically { n_journals_till_marked: usize },
    Manually,
}

impl From<Opened> for Option<usize> {
    fn from(opened: Opened) -> Self {
        match opened {
            Opened::Automatically { n_journals_till_marked } => Some(n_journals_till_marked),
            Opened::Manually => None,
        }
    }
}

impl From<Option<usize>> for Opened {
    fn from(option: Option<usize>) -> Self {
        match option {
            Some(n_journals_till_marked) => Self::Automatically { n_journals_till_marked },
            None => Self::Manually,
        }
    }
}

impl Opened {
    pub fn next(self) -> Self {
        Option::<usize>::from(self)
            .map(|n| n.saturating_sub(1))
            .into()
    }

    pub fn prev(self) -> Self {
        Option::<usize>::from(self)
            .map(|n| n.saturating_add(1))
            .into()
    }
}

pub enum AppMarkingState {
    JournalReadyToQueue,
    JournalLoading,
    JournalLoaded,
    Marking { choices: ChoiceSelections },
    WaitingToGoBack { back: JournalTag },
    WaitingToReturn,
    Returning { task: Task<FetchJournalsOutput> },
}

impl<B> AppMarking<B> {
    pub async fn new(
        globals: Globals,
        auth: Authentication,
        assignment: String,
        journals: Journals,
        live_journal_tag: JournalTag,
        opened: Opened,
        tmux_side_pane: Option<TmuxPane>,
    ) -> Self {
        let choice_selections = ChoiceSelections::new(globals.choices());

        let opened = match opened {
            Opened::Automatically { .. } => opened,
            Opened::Manually => {
                let n_journals_till_marked = Self::calculate_n_journals_till_marked(opened, &journals, &live_journal_tag).await;
                if n_journals_till_marked == 0 {
                    opened
                } else {
                    Opened::Automatically { n_journals_till_marked }
                }
            }
        };

        Self {
            globals,
            auth,
            assignment,
            journals,
            live_journal_tag,
            opened,
            tmux_side_pane,
            state: AppMarkingState::JournalReadyToQueue,
            ui: MarkingUi::new(),
        }
    }

    pub fn globals(&self) -> &Globals {
        &self.globals
    }

    pub fn auth(&self) -> &Authentication {
        &self.auth
    }

    pub fn assignment(&self) -> &str {
        &self.assignment
    }

    pub fn journals(&self) -> &Journals {
        &self.journals
    }

    pub fn live_journal_tag(&self) -> &JournalTag {
        &self.live_journal_tag
    }
    
    pub fn state(&self) -> &AppMarkingState {
        &self.state
    }

    pub fn opened(&self) -> Opened {
        self.opened
    }

    async fn calculate_n_journals_till_marked(opened: Opened, journals: &Journals, live_journal_tag: &JournalTag) -> usize {
        if let Opened::Automatically { n_journals_till_marked } = opened {
            return n_journals_till_marked;
        }
        
        let mut iter = journals.iter();
        iter.find(|journal| journal.0 == live_journal_tag);

        let mut n_journals = 0;

        for journal in iter {
            if journal.1.lock().await.meta().mark().is_some() { break; }
            n_journals += 1;
        }

        n_journals
    }
}

#[async_trait]
impl<B: Backend + Send + 'static> AppPage<B> for AppMarking<B> {
    async fn tick(&mut self, io: Option<Event>) -> Result<Option<Box<dyn AppPage<B>>>> {
        match &mut self.state {
            AppMarkingState::JournalReadyToQueue | AppMarkingState::JournalLoading => {
                if self.journals.try_get(&self.live_journal_tag)
                    .map(|journal| journal.is_loaded())
                    .unwrap_or(false)
                {
                    self.state = AppMarkingState::JournalLoaded;
                } else if matches!(self.state, AppMarkingState::JournalReadyToQueue) {
                    self.journals.queue_load(self.live_journal_tag.clone(), self.globals.cgi_endpoint(), self.auth.clone(), self.globals.mark_name());

                    self.state = AppMarkingState::JournalLoading;
                }
            }
            AppMarkingState::JournalLoaded => {
                let journal = self.journals.get(&self.live_journal_tag)
                    .await
                    .expect("journal must exist in the database");
                
                let journal_meta = journal.meta();
                let journal_data = journal.data().expect("journal is loaded");

                let pid = process::id();
                let mut shell_command = self.globals().pager_command().to_string();
                for file in journal_data.submission_files().iter().chain(journal_data.marking_files()) {
                    let name = file.file_name();
                    let fd = file.file_data().as_raw_fd();

                    if Path::exists(Path::new(name)) || read_link(name).await.is_ok() {
                        remove_file(name).await?;
                    }

                    symlink(format!("/proc/{pid}/fd/{fd}"), name).await?;
                    shell_command += " ";
                    shell_command += name;
                }

                drop(journal);

                match self.tmux_side_pane.as_ref() {
                    Some(pane) => {
                        pane.respawn(&shell_command)?;
                    }
                    None => {
                        self.tmux_side_pane = Some(TmuxPane::new_from_split(&shell_command)?)
                    }
                }
    
                let choice_selections = ChoiceSelections::new(self.globals().choices());

                self.state = AppMarkingState::Marking { choices: choice_selections };

                let mut next_journals_iter = self.journals.iter();
                let _ = next_journals_iter.find(|(tag, _)| *tag == &self.live_journal_tag);

                let next_journals = next_journals_iter.take(self.globals.preload())
                    .map(|(tag, _)| tag.clone())
                    .collect::<Vec<_>>();

                for next_journal in next_journals {
                    self.journals.queue_load(next_journal, self.globals.cgi_endpoint(), self.auth.clone(), self.globals.mark_name());
                }
            }
            AppMarkingState::Marking { .. } => {}
            AppMarkingState::WaitingToGoBack { back } => {
                if self.journals.scan_queue()? == 0 {
                    // slow but safe
                    self.journals.unload(&back).await;

                    return Ok(Some(Box::new(
                        AppMarking::new(
                            self.globals.clone(),
                            self.auth.clone(),
                            mem::take(&mut self.assignment),
                            mem::take(&mut self.journals),
                            mem::take(back),
                            self.opened.prev(),
                            mem::take(&mut self.tmux_side_pane),
                        ).await
                    )));
                }
            }
            AppMarkingState::WaitingToReturn => {
                if self.journals.scan_queue()? == 0 {
                    let globals    = self.globals.clone();
                    let auth       = self.auth.clone();
                    let assignment = self.assignment.to_string();

                    let task = Task::new(
                        FetchJournalsTask {
                            globals,
                            auth,
                            assignment,
                        },
                        self.globals.panic_on_drop(),
                    );

                    self.state = AppMarkingState::Returning { task };
                }
            }
            AppMarkingState::Returning { task } => {
                if let Some(output) = task.poll()? {
                    return Ok(Some(Box::new(
                        AppJournalList::new(
                            self.globals.clone(),
                            self.auth.clone(),
                            output.assignment,
                            output.journals,
                        )
                    )));
                }

                return Ok(None);
            }
        }

        let n = self.journals.scan_queue()?;

        let event = match io {
            Some(event) => event,
            None => return Ok(None),
        };

        match &mut self.state {
            AppMarkingState::JournalReadyToQueue
            | AppMarkingState::JournalLoading
            | AppMarkingState::JournalLoaded => {}
            AppMarkingState::Marking { choices } => {
                match event {
                    Event::Key(key) => {
                        match (key.modifiers, key.code) {
                            (KeyModifiers::NONE, KeyCode::Down | KeyCode::Char('j'))  => {
                                choices.cursor_next();
                            }
                            (KeyModifiers::NONE, KeyCode::Up | KeyCode::Char('k'))   => {
                                choices.cursor_prev();
                            }
                            (KeyModifiers::NONE, KeyCode::Char(' ') | KeyCode::Right) => {
                                choices.toggle_selection();
                            }
                            (KeyModifiers::NONE, KeyCode::Char('q')) => {
                                self.state = AppMarkingState::WaitingToReturn;
                            }
                            (KeyModifiers::NONE, KeyCode::Char('b')) => {
                                let mut journals_iter = self.journals.iter();
                                journals_iter.find(|(tag, _)| *tag == self.live_journal_tag());
                                journals_iter.next_back();

                                let prev_journal = journals_iter.next_back();
                                match prev_journal {
                                    Some((tag, _)) => {
                                        self.state = AppMarkingState::WaitingToGoBack { back: tag.clone() };
                                    }
                                    None => {
                                        // just ignore the back input if there is no previous journal
                                    }
                                }
                            }
                            (KeyModifiers::NONE, KeyCode::Char('s')) => {
                                match self.opened {
                                    Opened::Automatically { n_journals_till_marked: 0 } => {
                                        self.state = AppMarkingState::WaitingToReturn;
                                    }
                                    _ => {
                                        let mut journals_iter = self.journals.iter();
                                        journals_iter.find(|(tag, _)| *tag == self.live_journal_tag());

                                        let next_journal = journals_iter.next();
                                        match next_journal {
                                            Some((tag, _)) => {
                                                let tag = tag.clone();
                                                drop(journals_iter);
                                                return Ok(Some(Box::new(
                                                    AppMarking::new(
                                                        self.globals.clone(),
                                                        self.auth.clone(),
                                                        mem::take(&mut self.assignment),
                                                        mem::take(&mut self.journals),
                                                        tag,
                                                        self.opened.next(),
                                                        mem::take(&mut self.tmux_side_pane),
                                                    ).await
                                                )));
                                            }
                                            None => {
                                                // should be impossible
                                                self.state = AppMarkingState::WaitingToReturn;
                                            }
                                        }
                                    }
                                }
                            }
                            (KeyModifiers::NONE, KeyCode::Char(c)) if HOTKEYS.contains(c) => {
                                let char_index = HOTKEYS.find(c).expect("Must be in HOTKEYS.");
                                if choices.try_cursor_set(char_index) {
                                    choices.toggle_selection();
                                }
                            }
                            (KeyModifiers::NONE, KeyCode::Enter) | (KeyModifiers::CONTROL, KeyCode::Char('j')) => {
                                self.journals.queue_mark(
                                    self.live_journal_tag.clone(),
                                    mem::take(choices),
                                    self.globals.cgi_endpoint(),
                                    self.auth().clone(),
                                    self.globals.mark_name(),
                                );

                                match self.opened {
                                    Opened::Automatically { n_journals_till_marked: 0 } => {
                                        self.state = AppMarkingState::WaitingToReturn;
                                    }
                                    _ => {
                                        let mut journals_iter = self.journals.iter();
                                        journals_iter.find(|(tag, _)| *tag == self.live_journal_tag());
        
                                        let next_journal = journals_iter.next();
                                        match next_journal {
                                            Some((tag, _)) => {
                                                let tag = tag.clone();
                                                drop(journals_iter);
        
                                                return Ok(Some(Box::new(
                                                    AppMarking::new(
                                                        self.globals.clone(),
                                                        self.auth.clone(),
                                                        mem::take(&mut self.assignment),
                                                        mem::take(&mut self.journals),
                                                        tag,
                                                        self.opened.next(),
                                                        mem::take(&mut self.tmux_side_pane),
                                                    ).await
                                                )));
                                            }
                                            None => {
                                                self.state = AppMarkingState::WaitingToReturn;
                                            }
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            AppMarkingState::WaitingToGoBack { .. }
            | AppMarkingState::WaitingToReturn
            | AppMarkingState::Returning { .. } => {}
        }
        
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame<B>) {
        self.ui.draw(self, frame);
        self.ui.update();
    }

    async fn quit(&mut self) -> Result<()> {
        while self.journals.scan_queue()? > 0 {
            tokio::task::yield_now().await;
        }

        Ok(())
    }
}
