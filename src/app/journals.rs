use std::mem;

use anyhow::Result;
use async_trait::async_trait;
use crossterm::event::{Event, KeyCode};
use tui::{backend::Backend, Frame};
use tui_input::{Input, InputResponse, backend::crossterm as tui_input_crossterm};

use crate::{imark::{Globals, Authentication, Journals, JournalTag}, ui::{AppPage, journals::JournalsUi, UiPage}, util::task::{Task, TaskRunner}};

use super::marking::{AppMarking, Opened};

pub struct AppJournalList<B> {
    globals: Globals,
    auth: Authentication,
    assignment: String,
    journals: Journals,
    journals_view: Vec<JournalTag>,
    current_index: usize,
    filter: Input,
    ui: JournalsUi<B>,
}

impl<B> AppJournalList<B> {
    pub fn new(globals: Globals, auth: Authentication, assignment: String, journals: Journals) -> Self {
        let journals_view = filter_journals(&journals, "").cloned().collect();

        Self {
            globals,
            auth,
            assignment,
            journals,
            journals_view,
            current_index: 0,
            filter: Input::default(),
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

    pub fn journals_view(&self) -> &[JournalTag] {
        &self.journals_view
    }

    pub fn current_index(&self) -> usize {
        self.current_index
    }
    
    pub fn filter(&self) -> &Input {
        &self.filter
    }
}

pub fn filter_journals<'j, 'f: 'j>(journals: &'j Journals, filter: &'f str) -> impl Iterator<Item = &'j JournalTag> {
    journals.iter()
        .filter(move |(tag, meta)| {
            tag.student_id().contains(filter)
            || if let Ok(meta) = meta.try_lock() {
                let meta = meta.meta();

                meta.name().to_uppercase().contains(&filter.to_uppercase())
                || meta.mark().map(|m| format!("{:>5.02}", m))
                    .or(meta.provisional_mark().map(|m| format!("{:>5.02}?", m)))
                    .map_or(false, |mark| mark.contains(&filter))
                || meta.notes().map_or(false, |notes| notes.to_uppercase().contains(&filter.to_uppercase()))
            } else {
                false
            }
        })
        .map(|(tag, meta)| tag)
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
                    KeyCode::Down => {
                        self.current_index = (self.current_index + 1) % self.journals_view.len();
                    }
                    KeyCode::Up => {
                        self.current_index = (self.current_index + self.journals_view.len() - 1) % self.journals_view.len();
                    }
                    KeyCode::Enter => {
                        let globals    = self.globals().clone();
                        let auth       = self.auth().clone();
                        let assignment = mem::take(&mut self.assignment);
                        let journals   = mem::take(&mut self.journals);
                        let live_journal_tag = self.journals_view.iter()
                            .nth(self.current_index)
                            .expect("journal cannot just disappear")
                            .clone();

                        return Ok(Some(Box::new(
                            AppMarking::new(
                                globals,
                                auth,
                                assignment,
                                journals,
                                live_journal_tag,
                                Opened::Manually,
                                None,
                            ).await
                        )));
                    }
                    other => {
                        if let Some(response) = tui_input_crossterm::to_input_request(event)
                            .and_then(|req| self.filter.handle(req)) {
                            match response {
                                InputResponse::StateChanged(state) if state.value => {
                                    self.journals_view = filter_journals(&self.journals, &self.filter.value()).cloned().collect();
                                    self.current_index = 0;
                                }
                                _ => {}
                            }
                        }

                    }
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
