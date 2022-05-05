use std::{marker::PhantomData, num::Wrapping};

use tui::{Frame, backend::Backend, widgets::{ListItem, List, Block, Borders, ListState, Paragraph}, style::{Style, Color, Modifier}, layout::{Layout, Direction, Constraint}};

use crate::app::marking::AppMarking;

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
        
    }

    fn update(&mut self) {
        self.ticker += 1;
    }
}
