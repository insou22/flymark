#![allow(unused)]

mod choices;
mod ui;

use anyhow::{Result, Context, bail};
use choices::{Choices, Choice};
use clap::Parser;
use tempfile::TempDir;
use tokio::{fs::File, io::AsyncReadExt};
use ui::AppParams;

#[derive(Parser, Debug)]
#[clap(version)]
pub struct Args {
    /// The endpoint you will use for marking (overrides the course + session args)
    #[clap(short, long)]
    endpoint: Option<String>,

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

    let work_dir = move_to_work_dir()
        .context("Failed to create temporary work directory")?;

    let launch_params = AppParams::new(&args, &endpoint, &choices, &work_dir);
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

fn move_to_work_dir() -> Result<TempDir> {
    let work_dir = tempfile::tempdir()?;
    std::env::set_current_dir(&work_dir)?;
    
    Ok(work_dir)
}
