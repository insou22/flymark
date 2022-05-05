use std::{marker::PhantomData, num::Wrapping};

use tui::{Frame, backend::Backend, widgets::{ListItem, List, Block, Borders, ListState, Paragraph}, style::{Style, Color, Modifier}, layout::{Layout, Direction, Constraint}};

use crate::app::journals::AppJournalList;

use super::UiPage;

pub struct JournalsUi<B> {
    _phantom: PhantomData<B>,
}

impl<B> JournalsUi<B> {
    pub fn new() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }
}

impl<B: Backend + Send + 'static> UiPage<B> for JournalsUi<B> {
    type App = AppJournalList<B>;

    fn draw(&self, app: &Self::App, frame: &mut Frame<B>)
    where
        B: Backend,
    {
        let size = frame.size();
        let mut list_items = Vec::new();
        
        for (tag, journal) in app.journals().iter() {
            let (mark, provisional_mark, name) = {
                let journal = journal.try_lock()
                    .expect("while selecting a journal, there cannot be any lock contention");

                (journal.meta().mark(), journal.meta().provisional_mark(), journal.meta().name().to_string())
            };

            let item = ListItem::new(
                format!(
                    "{} | {:6} | {}",
                    tag.student_id(),
                    mark.map(|m| format!("{:>5.02}", m))
                        .or(provisional_mark.map(|m| format!("{:>5.02}?", m)))
                        .unwrap_or_else(|| "".to_string()),
                    name,
                )
            );

            list_items.push(item);
        }

        let list = List::new(list_items)
            .block(
                Block::default()
                    .title("Choose a journal")
                    .borders(Borders::ALL)
            )
            .style(Style::default().fg(Color::White))
            .highlight_style(Style::default().add_modifier(Modifier::ITALIC))
            .highlight_symbol(">> ");
        
        let mut list_state = ListState::default();
        list_state.select(Some(app.current_index()));
        
        frame.render_stateful_widget(list, size, &mut list_state);
    }

    fn update(&mut self) {

    }
}
