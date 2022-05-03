use std::process::Stdio;

use anyhow::Result;
use tmux_interface::{Session, Sessions, SESSION_ALL};
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

pub fn find_session(target_name: &str) -> Result<Session> {
    for session in Sessions::get(SESSION_ALL)? {
        match session.name {
            Some(ref session_name) if session_name == target_name => return Ok(session),
            _ => {}
        }
    }

    Err(anyhow::anyhow!("Session not found with name {target_name:?}"))
}
