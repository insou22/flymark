use std::{marker::PhantomData, num::Wrapping};

use tui::{Frame, backend::Backend, widgets::{ListItem, List, Block, Borders, ListState, Paragraph}, style::{Style, Color, Modifier}, layout::{Layout, Direction, Constraint}};

use crate::app::{assignments::{AppPostAuth, AppPostAuthState}};

use super::UiPage;

pub struct AssignmentsUi<B> {
    ticker: Wrapping<u32>,
    _phantom: PhantomData<B>,
}

impl<B> AssignmentsUi<B> {
    pub fn new() -> Self {
        Self {
            ticker: Wrapping(0),
            _phantom: PhantomData,
        }
    }
}

impl<B: Backend + Send + 'static> UiPage<B> for AssignmentsUi<B> {
    type App = AppPostAuth<B>;

    fn draw(&self, app: &Self::App, frame: &mut Frame<B>)
    where
        B: Backend,
    {
        match app.state() {
            AppPostAuthState::SelectingAssignment => {
                let size = frame.size();
                
                let list_items = app.assignments().iter()
                    .map(|assignment| ListItem::new(assignment.as_str()))
                    .collect::<Vec<_>>();
    
                let list = List::new(list_items)
                    .block(
                        Block::default()
                            .title("Choose an assignment")
                            .borders(Borders::ALL)
                    )
                    .style(Style::default().fg(Color::White))
                    .highlight_style(Style::default().add_modifier(Modifier::ITALIC))
                    .highlight_symbol(">> ");
                
                let mut list_state = ListState::default();
                list_state.select(Some(app.current_assignment()));

                frame.render_stateful_widget(list, size, &mut list_state);
            }
            AppPostAuthState::LoadingJournals { .. } => {
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

                let loading = Paragraph::new(String::from("Loading") + &".".repeat((self.ticker.0 as usize % 81) / 27 + 1))
                    .block(
                        Block::default()
                        .borders(Borders::NONE)
                    );
                
                frame.render_widget(loading, chunks[1]);
            }
        }
    }

    fn update(&mut self) {
        self.ticker += 1;
    }
}
