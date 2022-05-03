use std::process::Stdio;

use anyhow::{Result, Context};
use tmux_interface::{Session, Sessions, SESSION_ALL, Window, ListWindows, Windows, WindowFlag, WINDOW_ALL, WINDOW_ACTIVE, TargetSession, Panes, TargetWindowExt, PANE_ALL, Pane};
use tokio::process::Command;

pub async fn get_tmux_session() -> Result<Session> {
    if !std::env::vars().any(|(arg, _)| arg == "TMUX") {
        return Err(anyhow::anyhow!("Not in tmux session (TMUX environment variable not set)"));
    }

    let output_bytes = Command::new("tmux")
        .args(["display-message", "-p", "#S"])
        .stdout(Stdio::piped())
        .spawn()?
        .wait_with_output().await?
        .stdout;
    
    let session_name = std::str::from_utf8(&output_bytes)?
        .trim();

    let session = find_session(session_name)?;

    Ok(session)
}

pub async fn get_active_window(session: &Session) -> Result<Window> {
    let mut windows = Windows::get(
        &TargetSession::Id(session.id.expect("tmux session should have id")),
        WINDOW_ALL
    ).context("Failed to find active tmux windows")?;

    let mut active_windows = windows.0.iter()
        .filter(|window| window.active == Some(true));
    
    let hopefully_only_active_window = active_windows.next();

    match (hopefully_only_active_window, active_windows.next()) {
        (None, None) => {
            Err(anyhow::anyhow!("No active tmux windows"))
        }
        (Some(active_window), None) => {
            Ok(active_window.clone())
        }
        (Some(active_window), Some(_)) => {
            Err(anyhow::anyhow!("Multiple active tmux windows"))
        }
        (None, Some(_)) => {
            Err(anyhow::anyhow!("Wow that's some unfused fuckery right there"))
        }
    }
}

pub async fn get_active_pane(session: &Session, window: &Window) -> Result<Pane> {
    let mut panes = Panes::get(
        &TargetWindowExt::id(
            Some(&TargetSession::Id(session.id.expect("tmux session should have id"))),
            window.id.expect("tmux window should have id")
        ),
        PANE_ALL
    ).context("Failed to find active tmux panes")?;

    let mut active_panes = panes.0.iter()
        .filter(|pane| pane.active == Some(true));
    
    let hopefully_only_active_pane = active_panes.next();

    match (hopefully_only_active_pane, active_panes.next()) {
        (None, None) => {
            Err(anyhow::anyhow!("No active tmux panes"))
        }
        (Some(active_pane), None) => {
            Ok(active_pane.clone())
        }
        (Some(active_window), Some(_)) => {
            Err(anyhow::anyhow!("Multiple active tmux panes"))
        }
        (None, Some(_)) => {
            Err(anyhow::anyhow!("Wow that's some unfused fuckery right there"))
        }
    }
}

pub fn find_session(target_name: &str) -> Result<Session> {
    for session in Sessions::get(SESSION_ALL)? {
        match session.name {
            Some(ref session_name) if session_name == target_name => return Ok(session),
            _ => {}
        }
    }

    Err(anyhow::anyhow!("Session not found with name {target_name:?}"))
}
