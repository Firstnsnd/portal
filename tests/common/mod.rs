//! Test utilities and helpers for Portal project

use std::time::Duration;

/// Timeout for async operations in tests
pub const TEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Create a test tokio runtime
pub fn create_test_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().expect("Failed to create test runtime")
}

/// Temporary directory for test files
pub struct TempDir {
    path: std::path::PathBuf,
    keep_on_drop: bool,
}

impl TempDir {
    /// Create a new temporary directory
    pub fn new() -> Self {
        let path = std::env::temp_dir()
            .join(format!("portal-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).expect("Failed to create temp dir");
        Self {
            path,
            keep_on_drop: false,
        }
    }

    /// Get the path to the temporary directory
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Create a file in the temp directory
    pub fn write_file(&self, name: &str, content: &str) -> std::path::PathBuf {
        let file_path = self.path.join(name);
        std::fs::write(&file_path, content).expect("Failed to write file");
        file_path
    }

    /// Read a file from the temp directory
    pub fn read_file(&self, name: &str) -> String {
        let file_path = self.path.join(name);
        std::fs::read_to_string(&file_path).expect("Failed to read file")
    }

    /// Keep the temp directory when dropped (for debugging)
    pub fn keep_on_drop(&mut self) {
        self.keep_on_drop = true;
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if !self.keep_on_drop {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_temp_dir_creation() {
        let temp = TempDir::new();
        assert!(temp.path().exists());

        temp.write_file("test.txt", "Hello, World!");
        assert!(temp.path().join("test.txt").exists());

        let content = temp.read_file("test.txt");
        assert_eq!(content, "Hello, World!");
    }
}
