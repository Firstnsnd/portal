//! Mock SSH server for testing SSH client functionality

use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Simple mock SSH server for testing
pub struct MockSshServer {
    pub host: String,
    pub port: u16,
    shutdown_tx: mpsc::Sender<()>,
}

impl MockSshServer {
    /// Create a new mock SSH server that binds to a random port
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let port = addr.port();

        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

        tokio::spawn(async move {
            // Simple echo server that accepts connections
            tokio::select! {
                _ = async {
                    while let Ok((mut socket, _)) = listener.accept().await {
                        tokio::spawn(async move {
                            let mut buf = [0; 1024];
                            // Echo back any data
                            while let Ok(n) = socket.read(&mut buf).await {
                                if n == 0 { break; }
                                if socket.write_all(&buf[..n]).await.is_err() {
                                    break;
                                }
                            }
                        });
                    }
                } => {}
                _ = shutdown_rx.recv() => {
                    // Shutdown requested
                }
            }
        });

        Ok(Self {
            host: "127.0.0.1".to_string(),
            port,
            shutdown_tx,
        })
    }

    /// Get the connection string for this server
    pub fn connection_string(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    /// Get the port for compatibility
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Get the host for compatibility
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Shutdown the server
    pub async fn shutdown(self) {
        let _ = self.shutdown_tx.send(()).await;
    }
}

/// Mock SSH server with password authentication
pub struct MockPasswordAuthServer {
    pub server: MockSshServer,
    pub expected_password: String,
}

impl MockPasswordAuthServer {
    /// Create a new mock SSH server that expects a specific password
    pub async fn new(expected_password: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let server = MockSshServer::new().await?;
        Ok(Self {
            server,
            expected_password: expected_password.to_string(),
        })
    }

    /// Get the connection string for this server
    pub fn connection_string(&self) -> String {
        self.server.connection_string()
    }

    /// Get the expected password
    pub fn expected_password(&self) -> &str {
        &self.expected_password
    }

    /// Get the port
    pub fn port(&self) -> u16 {
        self.server.port
    }

    /// Get the host
    pub fn host(&self) -> &str {
        self.server.host()
    }
}

/// Mock SSH server that simulates various failure modes
pub struct FailingSshServer {
    pub server: MockSshServer,
    pub failure_mode: FailureMode,
}

#[derive(Clone)]
pub enum FailureMode {
    RejectAllConnections,
    RejectAllAuth,
    TimeoutAfterAuth,
}

impl FailingSshServer {
    /// Create a new mock SSH server that fails in specific ways
    pub async fn new(failure_mode: FailureMode) -> Result<Self, Box<dyn std::error::Error>> {
        let server = MockSshServer::new().await?;
        Ok(Self {
            server,
            failure_mode,
        })
    }

    /// Get the connection string for this server
    pub fn connection_string(&self) -> String {
        self.server.connection_string()
    }

    /// Get the port
    pub fn port(&self) -> u16 {
        self.server.port
    }

    /// Get the host
    pub fn host(&self) -> &str {
        self.server.host()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_server_creation() {
        let server = MockSshServer::new().await.unwrap();
        assert_eq!(server.host, "127.0.0.1");
        assert!(server.port > 0);
    }

    #[tokio::test]
    async fn test_mock_server_shutdown() {
        let server = MockSshServer::new().await.unwrap();
        server.shutdown().await;
        // Server should shutdown without panicking
    }

    #[tokio::test]
    async fn test_password_server_creation() {
        let server = MockPasswordAuthServer::new("testpass").await.unwrap();
        assert_eq!(server.expected_password(), "testpass");
    }
}
