use std::{marker::PhantomData, num::Wrapping};

use tui::{Frame, backend::Backend, layout::{Layout, Direction, Constraint}, widgets::{Paragraph, Block, Borders}, style::{Style, Color}};

use crate::app::auth::{AppPreAuth, AppPreAuthState};

use super::UiPage;

pub struct AuthUi<B> {
    ticker: Wrapping<u32>,
    _phantom: PhantomData<B>,
}

impl<B> AuthUi<B> {
    pub fn new() -> Self {
        Self {
            ticker: Wrapping(0),
            _phantom: PhantomData,
        }
    }
}

impl<B: Backend + Send + 'static> UiPage<B> for AuthUi<B> {
    type App = AppPreAuth<B>;

    fn draw(&self, app: &Self::App, frame: &mut Frame<B>)
    where
        B: Backend,
    {
        let size = frame.size();

        const INPUT_HEIGHT: u16 = 3;
        const INPUT_WIDTH:  u16 = 30;

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
                    Constraint::Length(size.height.saturating_sub(INPUT_HEIGHT * 2) / 2),
                    Constraint::Length(INPUT_HEIGHT),
                    Constraint::Length(INPUT_HEIGHT),
                    Constraint::Length(size.height.saturating_sub(INPUT_HEIGHT * 2) / 2),
                ]
            )
            .split(chunks[1]);

        match app.state() {
            AppPreAuthState::EnteringZid { zid_input } => {
                let zid_paragraph = Paragraph::new(zid_input.value())
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("zID")
                            .border_style(Style::default().fg(Color::LightGreen))
                        );

                frame.render_widget(zid_paragraph, chunks[1]);
                
                let password_paragraph = Paragraph::new("")
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("password")
                    );

                frame.render_widget(password_paragraph, chunks[2]);

                frame.set_cursor(
                    chunks[1].x + zid_input.cursor() as u16 + 1,
                    chunks[1].y + 1,
                )
            }
            AppPreAuthState::EnteringPassword { zid, password_input } => {
                let zid_paragraph = Paragraph::new(zid.as_str())
                    .block(
                        Block::default()
                        .borders(Borders::ALL)
                        .title("zID")
                    );

                frame.render_widget(zid_paragraph, chunks[1]);

                let password_paragraph = Paragraph::new(
                        String::from("*").repeat(password_input.value().len())
                    )
                    .block(
                        Block::default()
                        .borders(Borders::ALL)
                        .title("password")
                        .border_style(Style::default().fg(Color::LightGreen))
                    );

                frame.render_widget(password_paragraph, chunks[2]);

                frame.set_cursor(
                    chunks[2].x + password_input.cursor() as u16 + 1,
                    chunks[2].y + 1,
                )

            }
            AppPreAuthState::Authenticating { .. } => {
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
