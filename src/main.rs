#![allow(unused)]

mod app;
mod choice;
mod imark;
mod term;
mod ui;
mod util;

use std::process::Stdio;

use anyhow::{Result, bail, Context};
use choice::{Choices, Choice};
use clap::Parser;
use imark::Globals;
use tempfile::TempDir;
use tokio::{process::Command, fs::File, io::AsyncReadExt};

#[derive(Parser, Debug)]
#[clap(version)]
pub struct Args {
    /// The cgi endpoint you will use for marking (overrides the course + session args).
    /// Generally not required.
    #[clap(short('e'), long)]
    cgi_endpoint: Option<String>,

    /// Command to run the marking pager (default: tries to find bat, falls back to less)
    #[clap(short, long)]
    pager_command: Option<String>,

    /// The path to the marking scheme you will use
    scheme: String,

    /// Course (format: cs1521)
    course: String,

    /// Session (format: 22T1)
    session: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let cgi_endpoint = get_cgi_endpoint(&args);
    let choices  = get_choices(&args.scheme).await
        .with_context(|| format!("Failed to read scheme file: {}", args.scheme))?;

    ensure_tmux()?;

    let pager_command = locate_pager(&args).await?;

    let _work_dir = move_to_work_dir()
        .context("Failed to create temporary work directory")?;
    
    let globals = Globals::new(cgi_endpoint, pager_command, choices);
    
    ui::launch(globals).await?;

    println!("Thanks for using flymark!");
    Ok(())
}

fn get_cgi_endpoint(args: &Args) -> String {
    args.cgi_endpoint
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

    let choices = choice::parse_choices(&contents)?;

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
