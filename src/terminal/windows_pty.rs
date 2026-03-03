//! Windows PTY implementation using ConPTY

use super::{Error, Pty, PtySize, Result};

/// Windows PTY implementation using ConPTY
pub struct WindowsPty {
    // ConPTY implementation placeholder
    // Windows requires complex ConPTY setup with pseudoconsole
    alive: bool,
}

impl Pty for WindowsPty {
    fn spawn(command: &str, args: &[&str], _size: PtySize) -> Result<Self> {
        // TODO: Implement ConPTY spawn
        // Requires:
        // - CreatePseudoConsole
        // - CreateProcess with STARTUPINFOEX
        // - Pipe creation for stdin/stdout

        Err(Error::SpawnFailed(
            "Windows ConPTY not yet implemented".to_string(),
        ))
    }

    fn write(&mut self, _data: &[u8]) -> Result<()> {
        if !self.alive {
            return Err(Error::AlreadyClosed);
        }
        Ok(())
    }

    fn try_read(&mut self) -> Result<Vec<u8>> {
        if !self.alive {
            return Ok(Vec::new());
        }
        Ok(Vec::new())
    }

    fn resize(&mut self, _size: PtySize) -> Result<()> {
        if !self.alive {
            return Err(Error::AlreadyClosed);
        }
        Ok(())
    }

    fn is_alive(&self) -> bool {
        self.alive
    }

    fn try_wait(&mut self) -> Result<Option<std::process::ExitStatus>> {
        Ok(None)
    }

    fn kill(&mut self) -> Result<()> {
        self.alive = false;
        Ok(())
    }
}
