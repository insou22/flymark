use std::{process, path::Path, os::unix::prelude::AsRawFd};

use anyhow::Result;
use async_trait::async_trait;
use crossterm::event::Event;
use tmux_interface::{RespawnPane, SplitWindow};
use tokio::fs::{remove_file, symlink};
use tui::{backend::Backend, Frame};

use crate::{imark::{Globals, Authentication, Journals, JournalTag}, choice::ChoiceSelections, ui::{marking::MarkingUi, AppPage, UiPage}};

pub struct AppMarking<B> {
    globals: Globals,
    auth: Authentication,
    assignment: String,
    journals: Journals,
    live_journal_tag: JournalTag,
    tmux_side_pane_id: Option<String>,
    state: AppMarkingState,
    ui: MarkingUi<B>,
}

pub enum AppMarkingState {
    JournalReadyToQueue,
    JournalLoading,
    JournalLoaded,
    Marking { choices: ChoiceSelections },
}

impl<B> AppMarking<B> {
    pub fn new(
        globals: Globals,
        auth: Authentication,
        assignment: String,
        journals: Journals,
        live_journal_tag: JournalTag,
        tmux_side_pane_id: Option<String>,
    ) -> Self {
        let choice_selections = ChoiceSelections::new(globals.choices()); 

        Self {
            globals,
            auth,
            assignment,
            journals,
            live_journal_tag,
            tmux_side_pane_id,
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
}

#[async_trait]
impl<B: Backend + Send + 'static> AppPage<B> for AppMarking<B> {
    async fn tick(&mut self, io: Option<Event>) -> Result<Option<Box<dyn AppPage<B>>>> {
        match &mut self.state {
            AppMarkingState::JournalReadyToQueue | AppMarkingState::JournalLoading => {
                if self.journals.get(&self.live_journal_tag).await
                    .expect("journal must exist in the database")
                    .is_loaded()
                {
                    self.state = AppMarkingState::JournalLoaded;
                } else if matches!(self.state, AppMarkingState::JournalReadyToQueue) {
                    self.journals.queue_load(&self.live_journal_tag, self.globals.cgi_endpoint(), &self.auth);

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

                    if Path::exists(Path::new(name)) {
                        remove_file(name).await?;
                    }

                    symlink(format!("/proc/{pid}/fd/{fd}"), name).await?;
                    shell_command += " ";
                    shell_command += name;
                }

                drop(journal);

                match self.tmux_side_pane_id.as_ref() {
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
        
                        self.tmux_side_pane_id = Some(pane_id);
                    }
                }
    
                let choice_selections = ChoiceSelections::new(self.globals().choices());

                self.state = AppMarkingState::Marking { choices: choice_selections };

                let mut next_three_journals_iter = self.journals.iter();
                let _ = next_three_journals_iter.find(|(tag, _)| *tag == &self.live_journal_tag);

                let next_three_journals = next_three_journals_iter.take(3)
                    .map(|(tag, _)| tag.clone())
                    .collect::<Vec<_>>();
                
                for next_journal in next_three_journals {
                    self.journals.queue_load(&next_journal, self.globals.cgi_endpoint(), &self.auth);
                }
            }
            AppMarkingState::Marking { .. } => {}
        }
        
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame<B>) {
        self.ui.draw(self, frame);
        self.ui.update();
    }

    async fn quit(&mut self) {
        
    }
}
