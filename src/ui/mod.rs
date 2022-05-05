pub mod auth;
pub mod assignments;
pub mod journals;
pub mod marking;

use std::time::Duration;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use crossterm::event::{Event, EventStream, KeyModifiers, KeyCode};
use futures::{FutureExt, StreamExt};
use futures_timer::Delay;
use tokio::select;
use tui::{Frame, backend::{Backend, CrosstermBackend}, Terminal};

use crate::{app::auth::AppPreAuth, imark::Globals, term::TerminalSettings};

#[async_trait]
pub trait AppPage<B> {
    async fn tick(&mut self, io: Option<Event>) -> Result<Option<Box<dyn AppPage<B>>>>
    where
        B: Backend + Send + 'static,
    ;

    fn draw(&mut self, frame: &mut Frame<B>)
    where
        B: Backend,
    ;

    async fn quit(&mut self) -> Result<()>;
}

pub trait UiPage<B> {
    type App: AppPage<B>;
    
    fn draw(&self, app: &Self::App, frame: &mut Frame<B>)
    where
        B: Backend
    ;

    fn update(&mut self);
}

pub async fn launch(globals: Globals) -> Result<()> {
    let mut terminal = TerminalSettings::mangle_terminal(std::io::stdout(), CrosstermBackend::new)?;

    main_loop(terminal.terminal_mut(), globals).await?;

    Ok(())
}

async fn main_loop<B: Backend + Send + 'static>(terminal: &mut Terminal<B>, globals: Globals) -> Result<()> {
    let mut event_reader = EventStream::new();
    let mut app: Box<dyn AppPage<B>> = Box::new(AppPreAuth::<B>::new(globals));

    loop {
        let timeout = Delay::new(Duration::from_millis(10)).fuse();
        let event   = event_reader.next().fuse();

        let event = select! {
            _     = timeout => None,
            event = event   => {
                let event: Result<_> = event.ok_or(anyhow!("Couldn't read input from terminal")).into();
                let event = event??;
                Some(event)
            }
        };

        if should_quit(event) {
            app.quit().await;
            break;
        }

        if let Some(new_page) = app.tick(event).await? {
            app = new_page;
        }

        terminal.draw(|frame| app.draw(frame))?;
    }

    Ok(())
}

fn should_quit(event: Option<Event>) -> bool {
    match event {
        Some(Event::Key(key)) => {
            let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

            match key.code {
                KeyCode::Char('c') if ctrl => {
                    return true;
                }
                _ => {}
            }
        }
        _ => {}
    }

    false
}
