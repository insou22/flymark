use std::mem;

use anyhow::Result;
use async_trait::async_trait;
use crossterm::event::{Event, KeyCode};
use tui::{backend::Backend, Frame};

use crate::{imark::{Globals, Authentication, Journals}, ui::{AppPage, journals::JournalsUi, UiPage}, util::task::{Task, TaskRunner}};

use super::marking::AppMarking;

pub struct AppJournalList<B> {
    globals: Globals,
    auth: Authentication,
    assignment: String,
    journals: Journals,
    current_index: usize,
    ui: JournalsUi<B>,
}

impl<B> AppJournalList<B> {
    pub fn new(globals: Globals, auth: Authentication, assignment: String, journals: Journals) -> Self {
        Self {
            globals,
            auth,
            assignment,
            journals,
            current_index: 0,
            ui: JournalsUi::new(),
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

    pub fn current_index(&self) -> usize {
        self.current_index
    }
}

#[async_trait]
impl<B: Backend + Send + 'static> AppPage<B> for AppJournalList<B> {
    async fn tick(&mut self, io: Option<Event>) -> Result<Option<Box<dyn AppPage<B>>>> {
        let event = match io {
            Some(event) => event,
            None => return Ok(None),
        };

        match event {
            Event::Key(key) => {
                match key.code {
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.current_index = (self.current_index + 1) % self.journals.len();
                    }
                    KeyCode::Up | KeyCode::Char('k')    => {
                        self.current_index = (self.current_index + self.journals.len() - 1) % self.journals.len();
                    }
                    KeyCode::Enter => {
                        let globals    = self.globals().clone();
                        let auth       = self.auth().clone();
                        let assignment = mem::take(&mut self.assignment);
                        let journals   = mem::take(&mut self.journals);
                        let live_journal_tag = journals.iter()
                            .nth(self.current_index)
                            .expect("journal cannot just disappear")
                            .0
                            .clone();

                        return Ok(Some(Box::new(
                            AppMarking::new(
                                globals,
                                auth,
                                assignment,
                                journals,
                                live_journal_tag,
                                None,
                            )
                        )));
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
        while self.journals.scan_queue()? > 0 {
            tokio::task::yield_now().await;
        }

        Ok(())
    }
}
