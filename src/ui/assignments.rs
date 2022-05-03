use anyhow::Result;
use crossterm::event::{Event, KeyEvent, KeyCode};
use tui::{Frame, backend::Backend, layout::{Layout, Direction, Constraint, Alignment}, widgets::{Paragraph, Block, Borders, List, ListItem, ListState}, style::{Style, Color, Modifier}};

use super::{App, UiTickers, AppState, journals::JournalsState};

pub enum AssignmentsState {
    ChoosingAssignment { assignments: Vec<String>, list_state: ListState },
}

impl AssignmentsState {
    pub fn new(assignments: Vec<String>) -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        
        Self::ChoosingAssignment {
            assignments,
            list_state,
        }
    }
}

pub fn tick_app(app: &mut App<'_>, io_event: Option<Event>) -> Result<()> {
    let event = match io_event {
        Some(event) => event,
        None => return Ok(()),
    };

    match &mut app.state {
        AppState::Choosing(AssignmentsState::ChoosingAssignment { assignments, list_state }) => {
            match event {
                Event::Key(key) => {
                    match key.code {
                        KeyCode::Down  => {
                            let current = list_state.selected().unwrap();
                            
                            list_state.select(Some(
                                if current == assignments.len() - 1 {
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
                                    assignments.len() - 1
                                } else {
                                    current - 1
                                }
                            ));
                        }
                        KeyCode::Enter => {
                            app.state = AppState::Journals(JournalsState::new());
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        _ => unreachable!(),
    }
    
    Ok(())
}

pub fn draw<B: Backend>(frame: &mut Frame<B>, app: &mut App, tickers: &mut UiTickers) {
    match &mut app.state {
        AppState::Choosing(AssignmentsState::ChoosingAssignment { assignments, list_state }) => {
            let size = frame.size();
            
            let list_items = assignments.iter()
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

            // let loading = Paragraph::new(String::from("Nice one!"))
            //     .block(
            //         Block::default()
            //         .borders(Borders::NONE)
            //     )
            //     .alignment(Alignment::Left);
            
            frame.render_stateful_widget(list, size, list_state);
        }
        _ => unreachable!(),
    }
}
