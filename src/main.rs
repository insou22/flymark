// #![allow(unused)]

mod choices;
mod ui;

use std::process::Stdio;

use anyhow::{Result, Context, bail};
use choices::{Choices, Choice};
use clap::Parser;
use tempfile::TempDir;
use tokio::{fs::File, io::AsyncReadExt, process::Command};
use ui::AppParams;

#[derive(Parser, Debug)]
#[clap(version)]
pub struct Args {
    /// The endpoint you will use for marking (overrides the course + session args)
    #[clap(short, long)]
    endpoint: Option<String>,

    /// Command to run the marking pager (default: tries to find bat, falls back to less)
    #[clap(short, long)]
    pager_command: Option<String>,

    /// The path to the marking scheme you will use
    scheme: String,

    /// Course
    course: String,

    /// Session
    session: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let endpoint = get_endpoint(&args);
    let choices  = get_choices(&args.scheme).await
        .with_context(|| format!("Failed to read scheme file: {}", args.scheme))?;

    ensure_tmux()?;

    let pager_command = locate_pager(&args).await?;

    let work_dir = move_to_work_dir()
        .context("Failed to create temporary work directory")?;

    let launch_params = AppParams::new(
        &endpoint,
        &choices,
        &pager_command,
        &work_dir
    );
    ui::launch_ui(launch_params).await?;

    Ok(())
}

fn get_endpoint(args: &Args) -> String {
    args.endpoint
        .as_ref()
        .cloned()
        .unwrap_or_else(|| {
            let course  = args.course.as_str();
            let session = args.session.as_str();

            format!("https://cgi.cse.unsw.edu.au/~{course}/{session}/imark/server.cgi/")
        })
}

async fn get_choices(scheme: &str) -> Result<Choices> {
    let mut file = File::open(scheme).await?;

    let mut contents = String::new();
    file.read_to_string(&mut contents).await?;

    let choices = choices::parse_choices(&contents)?;

    let real_choice = choices.choices.iter()
        .find(|choice| !matches!(choice, Choice::Comment(_)));
    
    if real_choice.is_none() {
        bail!("Choice file must contain at least one *actual* choice");
    }

    Ok(choices)
}

fn ensure_tmux() -> Result<()> {
    if !std::env::vars().any(|(arg, _)| arg == "TMUX") {
        return Err(anyhow::anyhow!("Not in tmux session (TMUX environment variable not set)"));
    }

    Ok(())
}

async fn locate_pager(args: &Args) -> Result<String> {
    if let Some(pager) = args.pager_command.as_ref() {
        return Ok(pager.to_string());
    }

    let have_6991_bat = Command::new("/home/cs6991/bin/bat")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .arg("--version")
        .output()
        .await
        .is_ok();
    
    if have_6991_bat {
        return Ok("/home/cs6991/bin/bat --paging=always".to_string());
    }

    let have_bat = Command::new("bat")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .arg("--version")
        .output()
        .await
        .is_ok();
    
    if have_bat {
        return Ok("bat --paging=always".to_string());
    }
    
    let have_less = Command::new("less")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .arg("--version")
        .output()
        .await
        .is_ok();

    if have_less {
        Ok("less".to_string())
    } else {
        bail!("Failed to find a pager -- please specify one with -p");
    }
}

fn move_to_work_dir() -> Result<TempDir> {
    let work_dir = tempfile::tempdir()?;
    std::env::set_current_dir(&work_dir)?;
    
    Ok(work_dir)
}
