use std::borrow::Cow;

use anyhow::Result;
use tmux_interface::{SplitWindow, RespawnPane, KillPane};

pub struct TmuxPane {
    pane_id: String,
}

impl TmuxPane {
    pub fn new_from_split(shell_command: &str) -> Result<Self> {
        let pane_id = String::from_utf8(
            SplitWindow::new()
                .print()
                .horizontal()
                .detached()
                .shell_command(shell_command)
                .output()?
                .stdout()
        )?.trim().to_string();

        Ok(
            Self {
                pane_id
            }
        )
    }

    pub fn respawn(&self, shell_command: &str) -> Result<()> {
        RespawnPane::new()
            .kill()
            .target_pane(&self.pane_id)
            .shell_command(shell_command)
            .output()?;
        
        Ok(())
    }
}

impl Drop for TmuxPane {
    fn drop(&mut self) {
        KillPane::new()
            .target_pane(&self.pane_id)
            .output();
    }
}
