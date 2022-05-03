use anyhow::Result;
use crossterm::event::{Event, KeyEvent, KeyCode};
use tui::{Frame, backend::Backend, layout::{Layout, Direction, Constraint, Alignment}, widgets::{Paragraph, Block, Borders, List, ListItem, ListState}, style::{Style, Color, Modifier}};

use super::{App, UiTickers, AppState};

pub enum JournalsState {
    ChoosingJournal { },
}

impl JournalsState {
    pub fn new() -> Self {
        Self::ChoosingJournal {}
    }
}

pub fn tick_app(app: &mut App<'_>, io_event: Option<Event>) -> Result<()> {
    let event = match io_event {
        Some(event) => event,
        None => return Ok(()),
    };
    
    Ok(())
}

pub fn draw<B: Backend>(frame: &mut Frame<B>, app: &mut App, tickers: &mut UiTickers) {
    match &mut app.state {
        AppState::Journals(JournalsState::ChoosingJournal { }) => {
            let size = frame.size();

            let loading = Paragraph::new(String::from("Time to choose journal!"))
                .block(
                    Block::default()
                    .borders(Borders::NONE)
                )
                .alignment(Alignment::Left);
            
            frame.render_widget(loading, size);
        }
        _ => unreachable!(),
    }
}
