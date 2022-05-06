use std::{marker::PhantomData, num::Wrapping};

use tui::{Frame, backend::Backend, widgets::{ListItem, List, Block, Borders, ListState, Paragraph}, style::{Style, Color, Modifier}, layout::{Layout, Direction, Constraint, Rect}, text::Span};

use crate::{app::marking::{AppMarking, AppMarkingState}, choice::Choice, util::HOTKEYS};

use super::UiPage;

pub struct MarkingUi<B> {
    ticker: Wrapping<u32>,
    _phantom: PhantomData<B>,
}

impl<B> MarkingUi<B> {
    pub fn new() -> Self {
        Self {
            ticker: Wrapping(0),
            _phantom: PhantomData,
        }
    }
}

impl<B: Backend + Send + 'static> UiPage<B> for MarkingUi<B> {
    type App = AppMarking<B>;

    fn draw(&self, app: &Self::App, frame: &mut Frame<B>)
    where
        B: Backend,
    {
        match app.state() {
            AppMarkingState::JournalReadyToQueue
            | AppMarkingState::JournalLoading
            | AppMarkingState::JournalLoaded
            | AppMarkingState::WaitingToReturn
            | AppMarkingState::Returning { .. } => {
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
            AppMarkingState::Marking { choices: selections } => {
                let size = frame.size();
    
                let info = "Press <space> to toggle a choice\n\
                Press <up>/<down> to select a choice\n\
                Press <enter> to submit and move to next journal\n\
                Press <q> to return to the journal list";

                let info_height = info.lines().count() as u16;
                const MARGIN: u16 = 1;
    
                let [journal_info_chunk, _, info_chunk, _, selections_chunk] = 
                    <[Rect; 5]>::try_from(
                        Layout::default()
                            .direction(Direction::Vertical)
                            .constraints(
                                [
                                    Constraint::Length(2),
                                    Constraint::Length(MARGIN),
                                    Constraint::Length(info_height),
                                    Constraint::Length(MARGIN),
                                    Constraint::Length(size.height.saturating_sub(info_height)),
                                ]
                            )
                            .split(size)
                    ).expect("chunk split into three");

                let journal_info = {
                    if let Some(journal) = app.journals().try_get(app.live_journal_tag()) {
                        let mark             = journal.meta().mark();
                        let provisional_mark = journal.meta().provisional_mark();
                        let name             = journal.meta().name();
            
                        Paragraph::new(
                            format!(
                                "  zid   |  mark  | name\n\
                                {} | {:6} | {}",
                                app.live_journal_tag().student_id(),
                                mark.map(|m| format!("{:>5.02}", m))
                                    .or(provisional_mark.map(|m| format!("{:>5.02}?", m)))
                                    .unwrap_or_else(|| "".to_string()),
                                name,
                            )
                        )
                    } else {
                        Paragraph::new(String::from("Journal loading..."))
                    }
                };

                frame.render_widget(journal_info, journal_info_chunk);
    
                let info =
                    Paragraph::new(info)
                        .block(
                            Block::default()
                                .borders(Borders::NONE)
                        );
    
                frame.render_widget(info, info_chunk);

                let list_items = app.globals().choices().choices.iter()
                    .enumerate()
                    .map(|(index, choice)| {
                        let hotkey = {
                            selections.from_real_index(index)
                                .and_then(|(index, _)| HOTKEYS.chars().nth(index))
                        };

                        let hotkey_string = match hotkey {
                            Some(hotkey) => format!("[{}] ", hotkey),
                            None => String::new(),
                        };

                        ListItem::new(Span::styled(
                            match choice {
                                Choice::Plus (n, text) => {
                                    format!("{hotkey_string}+{n} {text}")
                                }
                                Choice::Minus(n, text) => {
                                    format!("{hotkey_string}-{n} {text}")
                                }
                                Choice::Set  (n, text) => {
                                    format!("{hotkey_string}{n} {text}")
                                }
                                Choice::Comment(text)  => {
                                    text.to_string()
                                }
                            },
                            match selections.from_real_index(index) {
                                Some((_, selection)) if selection.selected() => {
                                    Style::default()
                                        .bg(Color::White)
                                        .fg(Color::Black)
                                }
                                _ => Style::default()
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
                    .highlight_symbol(">> ");
                
                let mut list_state = ListState::default();
                list_state.select(Some(selections.real_cursor()));
    
                frame.render_stateful_widget(list, selections_chunk, &mut list_state);
            }
        }
    }

    fn update(&mut self) {
        self.ticker += 1;
    }
}
