/// Unit tests for configuration module

#[cfg(test)]
mod tests {
    use super::super::*;
    use serde_json;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_temp_file() -> (NamedTempFile, std::path::PathBuf) {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();
        (temp_file, path)
    }

    #[test]
    fn test_host_entry_serialization() {
        let host = HostEntry {
            name: "Test Server".to_string(),
            host: "example.com".to_string(),
            port: 2222,
            username: "testuser".to_string(),
            group: "Production".to_string(),
            tags: vec!["web".to_string(), "linux".to_string()],
            is_local: false,
            credential_id: Some("cred-123".to_string()),
            auth: AuthMethod::None,
            startup_commands: vec!["tmux attach".to_string()],
            agent_forwarding: true,
            jump_host: None,
            port_forwards: vec![],
        };

        // Test JSON serialization
        let json = serde_json::to_string(&host).unwrap();
        assert!(json.contains("Test Server"));
        assert!(json.contains("example.com"));
        assert!(json.contains("2222"));

        // Test JSON deserialization
        let parsed: HostEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, host.name);
        assert_eq!(parsed.host, host.host);
        assert_eq!(parsed.port, host.port);
        assert_eq!(parsed.username, host.username);
        assert_eq!(parsed.group, host.group);
        assert_eq!(parsed.tags, host.tags);
        assert_eq!(parsed.credential_id, host.credential_id);
    }

    #[test]
    fn test_forward_kind_display() {
        assert_eq!(ForwardKind::Local.to_string(), "L");
        assert_eq!(ForwardKind::Remote.to_string(), "R");
    }

    #[test]
    fn test_port_forward_config_display() {
        let config = PortForwardConfig {
            kind: ForwardKind::Local,
            local_host: "127.0.0.1".to_string(),
            local_port: 8080,
            remote_host: "localhost".to_string(),
            remote_port: 3000,
        };

        let display = format!("{}", config);
        assert!(display.contains("-L"));
        assert!(display.contains("127.0.0.1:8080"));
        assert!(display.contains("localhost:3000"));

        let remote_config = PortForwardConfig {
            kind: ForwardKind::Remote,
            local_host: "127.0.0.1".to_string(),
            local_port: 8080,
            remote_host: "0.0.0.0".to_string(),
            remote_port: 2222,
        };

        let remote_display = format!("{}", remote_config);
        assert!(remote_display.contains("-R"));
        assert!(remote_display.contains("0.0.0.0:2222"));
        assert!(remote_display.contains("127.0.0.1:8080"));
    }

    #[test]
    fn test_snippet_serialization() {
        let snippet = Snippet {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Deploy Script".to_string(),
            command: "kubectl apply -f deployment.yaml".to_string(),
            group: "Kubernetes".to_string(),
        };

        let json = serde_json::to_string(&snippet).unwrap();
        let parsed: Snippet = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.name, snippet.name);
        assert_eq!(parsed.command, snippet.command);
        assert_eq!(parsed.group, snippet.group);
    }

    #[test]
    fn test_load_snippets_returns_vec() {
        // load_snippets() returns a Vec (may be empty or contain existing snippets)
        let snippets = load_snippets();
        // Just verify it returns a vector without panicking
        let _snippet_count = snippets.len();
    }

    #[test]
    fn test_save_and_load_snippets() {
        let (_temp_file, path) = create_temp_file();

        let original = vec![
            Snippet {
                id: "1".to_string(),
                name: "Test Snippet".to_string(),
                command: "echo test".to_string(),
                group: "Test".to_string(),
            },
        ];

        save_snippets(&original);
        let loaded = load_snippets();

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "Test Snippet");
        assert_eq!(loaded[0].command, "echo test");
    }

    #[test]
    fn test_connection_record_serialization() {
        let record = ConnectionRecord {
            host_name: "Test Server".to_string(),
            host: "example.com".to_string(),
            port: 2222,
            username: "user".to_string(),
            timestamp: 1234567890,
            success: true,
        };

        let json = serde_json::to_string(&record).unwrap();
        let parsed: ConnectionRecord = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.host_name, record.host_name);
        assert_eq!(parsed.host, record.host);
        assert_eq!(parsed.port, record.port);
    }

    #[test]
    fn test_key_binding_serialization() {
        let binding = KeyBinding {
            action: ShortcutAction::SplitHorizontal,
            key: "D".to_string(),
            ctrl: false,
            alt: false,
            shift: false,
            command: true,
        };

        let json = serde_json::to_string(&binding).unwrap();
        let parsed: KeyBinding = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.action, ShortcutAction::SplitHorizontal);
        assert_eq!(parsed.key, "D");
        assert!(parsed.command);
        assert!(!parsed.ctrl); // Should remain false
        assert!(!parsed.alt);
        assert!(!parsed.shift);
    }

    #[test]
    fn test_default_shortcuts() {
        let shortcuts = default_shortcuts();
        assert!(!shortcuts.is_empty());

        // Check that common shortcuts exist
        assert!(shortcuts.iter().any(|k| k.action == ShortcutAction::SplitHorizontal));
        assert!(shortcuts.iter().any(|k| k.action == ShortcutAction::SplitVertical));
        assert!(shortcuts.iter().any(|k| k.action == ShortcutAction::NewTab));
        assert!(shortcuts.iter().any(|k| k.action == ShortcutAction::CloseTab));
    }

    #[test]
    fn test_settings_defaults() {
        let settings = PortalSettings::default();
        assert_eq!(settings.font_size, 14.0);
        assert_eq!(settings.language, "en");
        assert_eq!(settings.scrollback_limit_mb, 100);
        assert_eq!(settings.ssh_keepalive_interval, 30);
        assert!(!settings.keyboard_shortcuts.is_empty());
    }

    #[test]
    fn test_credential_new_password() {
        let cred = Credential::new_password("Test Password".to_string(), "testuser".to_string());
        assert_eq!(cred.name, "Test Password");
        assert!(matches!(cred.credential_type, CredentialType::Password { username: u } if u == "testuser"));
        assert!(!cred.id.is_empty());
        assert!(cred.created_at > 0);
    }

    #[test]
    fn test_credential_new_ssh_key() {
        let cred = Credential::new_ssh_key(
            "Test Key".to_string(),
            "~/.ssh/id_rsa".to_string(),
            false,
            true,
        );
        assert_eq!(cred.name, "Test Key");
        assert!(matches!(cred.credential_type,
            CredentialType::SshKey { key_path, key_in_keychain: false, has_passphrase: true }
            if key_path == "~/.ssh/id_rsa"));
        assert!(!cred.id.is_empty());
    }
}
