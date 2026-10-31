/// Unit tests for configuration module
#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::*;
    
    
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
            auto_record: false,
            jump_host: None,
            port_forwards: vec![],
            proxy: None,
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
        assert_eq!(ForwardKind::Dynamic.to_string(), "D");
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
    fn test_dynamic_forward_config() {
        // -D only uses the local side; remote fields stay empty
        let config = PortForwardConfig {
            kind: ForwardKind::Dynamic,
            local_host: "127.0.0.1".to_string(),
            local_port: 1080,
            remote_host: String::new(),
            remote_port: 0,
        };

        let display = format!("{}", config);
        assert!(display.contains("-D"));
        assert!(display.contains("127.0.0.1:1080"));
        assert!(display.contains("SOCKS5"));

        // serde roundtrip through hosts.json
        let json = serde_json::to_string(&config).unwrap();
        let parsed: PortForwardConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, config);
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
        let (_temp_file, path) = create_temp_file();
        // load_snippets() returns a Vec (may be empty for a fresh file)
        let snippets = load_snippets(&path);
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

        save_snippets(&path, &original);
        let loaded = load_snippets(&path);

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
        assert_eq!(settings.llm.backend, crate::llm::types::LlmBackend::OpenAiCompatible);
        assert!(!settings.llm.has_api_key);
        assert!(settings.terminal_background_image.is_none());
        assert!((settings.terminal_background_opacity - 0.35).abs() < 1e-6);
    }

    #[test]
    fn test_settings_background_image_additive_compat() {
        // Old settings.json files have no background fields at all — they
        // must still load, defaulting to disabled.
        let legacy: PortalSettings = serde_json::from_str(r#"{"font_size":14.0}"#).unwrap();
        assert!(legacy.terminal_background_image.is_none());
        assert!((legacy.terminal_background_opacity - 0.35).abs() < 1e-6);

        // New fields roundtrip through serde when present.
        let configured: PortalSettings = serde_json::from_str(
            r#"{"font_size":14.0,"terminal_background_image":"/tmp/x.png",
                "terminal_background_opacity":0.8}"#,
        )
        .unwrap();
        assert_eq!(configured.terminal_background_image.as_deref(), Some("/tmp/x.png"));
        assert!((configured.terminal_background_opacity - 0.8).abs() < 1e-6);
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

    #[test]
    fn test_agent_credential_serde_additive_compat() {
        let cred = Credential::new_agent("My Agent".to_string(), "ops".to_string());
        assert!(matches!(cred.credential_type,
            CredentialType::Agent { ref username } if username == "ops"));

        // Roundtrip keeps the "agent" tag.
        let json = serde_json::to_string(&cred).unwrap();
        assert!(json.contains(r#""type":"agent""#), "json: {json}");
        let parsed: Credential = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, cred.name);
        assert_eq!(parsed.credential_type, cred.credential_type);

        // Old credentials.json files (no agent variant) still load.
        let legacy: Credential = serde_json::from_str(
            r#"{"id":"abc","name":"old","credential_type":{"type":"password","username":"u"},"created_at":1}"#,
        )
        .unwrap();
        assert!(matches!(legacy.credential_type, CredentialType::Password { .. }));

        // Agent JSON with a missing username field defaults to empty.
        let no_user: Credential = serde_json::from_str(
            r#"{"id":"d","name":"agent cred","credential_type":{"type":"agent"},"created_at":2}"#,
        )
        .unwrap();
        assert!(matches!(no_user.credential_type, CredentialType::Agent { username } if username.is_empty()));
    }

    #[test]
    fn test_resolve_agent_credential_and_legacy() {
        // Agent credential resolves without any keychain access.
        let cred = Credential::new_agent("a".to_string(), String::new());
        assert!(matches!(resolve_credential(&cred), ResolvedAuth::Agent));

        // Legacy host-level AuthMethod::Agent resolves the same.
        let mut host = HostEntry::new_local();
        host.auth = AuthMethod::Agent;
        assert!(matches!(resolve_auth(&host, &[]), ResolvedAuth::Agent));

        // And serializes with the agent tag for the legacy hosts.json path.
        let json = serde_json::to_string(&host.auth).unwrap();
        assert_eq!(json, r#"{"type":"agent"}"#);
        let back: AuthMethod = serde_json::from_str(&json).unwrap();
        assert_eq!(back, AuthMethod::Agent);
    }

    #[test]
    fn test_resolve_key_path_credential_reads_file() {
        // A key credential stored by file path (key_in_keychain=false) must
        // read the key at resolve time — previously key_path was ignored and
        // every path-based credential resolved to No authentication.
        let dir = std::env::temp_dir().join(format!("portal_key_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("id_test_key");
        std::fs::write(&path, "FAKE-KEY-CONTENT\n").unwrap();

        let cred = Credential::new_ssh_key(
            "path cred".to_string(),
            path.to_string_lossy().to_string(),
            false,
            false,
        );
        match resolve_credential(&cred) {
            ResolvedAuth::Key { key_content, passphrase } => {
                assert_eq!(key_content, "FAKE-KEY-CONTENT\n");
                assert!(passphrase.is_none());
            }
            other => panic!("expected ResolvedAuth::Key, got {:?}", other),
        }

        // A missing file and an empty path both degrade to None (not a panic
        // or a Key with empty content).
        let missing = Credential::new_ssh_key(
            "missing".to_string(),
            dir.join("nope").to_string_lossy().to_string(),
            false,
            false,
        );
        assert!(matches!(resolve_credential(&missing), ResolvedAuth::None));
        let empty = Credential::new_ssh_key("empty".to_string(), String::new(), false, false);
        assert!(matches!(resolve_credential(&empty), ResolvedAuth::None));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_expand_key_path() {
        // Non-tilde paths pass through untouched.
        let p = expand_key_path("/abs/key");
        assert_eq!(p, std::path::PathBuf::from("/abs/key"));
        // A bare `~` (no slash) must not panic and stays untouched.
        let p = expand_key_path("~");
        assert_eq!(p, std::path::PathBuf::from("~"));
        // `~/…` expands under the home directory.
        if let Some(home) = dirs::home_dir() {
            let p = expand_key_path("~/.ssh/id_ed25519");
            assert_eq!(p, home.join(".ssh/id_ed25519"));
        }
    }

    #[test]
    fn test_proxy_config_serde_additive_compat() {
        // Old hosts.json entries have no proxy field at all — they must
        // still load, defaulting to direct connection.
        let legacy: HostEntry = serde_json::from_str(
            r#"{"name":"old","host":"h","port":22,"username":"u","group":"","tags":[],
                "is_local":false,"auth":{"type":"none"},"startup_commands":[],
                "agent_forwarding":false,"auto_record":false,"port_forwards":[]}"#,
        )
        .unwrap();
        assert!(legacy.proxy.is_none());
        assert!(resolve_proxy(&legacy).is_none());

        // Full proxy roundtrip keeps kind + endpoint; the keychain flag too.
        let mut host = HostEntry::new_ssh(
            "via-proxy".to_string(),
            "target.example.com".to_string(),
            22,
            "u".to_string(),
            String::new(),
            None,
            Vec::new(),
        );
        host.proxy = Some(ProxyConfig {
            kind: ProxyKind::Socks5,
            host: "127.0.0.1".to_string(),
            port: 1080,
            username: "ops".to_string(),
            password_in_keychain: true,
        });
        let json = serde_json::to_string(&host).unwrap();
        assert!(json.contains(r#""type":"socks5""#), "json: {json}");
        assert!(json.contains("1080"));
        // The password must never appear in JSON even if someone stuffed it in.
        let parsed: HostEntry = serde_json::from_str(&json).unwrap();
        let proxy = parsed.proxy.unwrap();
        assert_eq!(proxy.kind, ProxyKind::Socks5);
        assert_eq!(proxy.port, 1080);
        assert!(proxy.password_in_keychain);
    }

    #[test]
    fn test_resolve_proxy_guards_incomplete_config() {
        // Missing endpoint fields → not resolvable, not a connect error.
        let mut host = HostEntry::new_local();
        host.proxy = Some(ProxyConfig {
            kind: ProxyKind::Http,
            host: String::new(),
            port: 0,
            username: String::new(),
            password_in_keychain: false,
        });
        assert!(resolve_proxy(&host).is_none());

        // Valid endpoint → resolved, empty username → None (not Some("")).
        host.proxy = Some(ProxyConfig {
            kind: ProxyKind::Http,
            host: "127.0.0.1".to_string(),
            port: 3128,
            username: String::new(),
            password_in_keychain: false,
        });
        let resolved = resolve_proxy(&host).unwrap();
        assert_eq!(resolved.kind, ProxyKind::Http);
        assert_eq!(resolved.port, 3128);
        assert!(resolved.username.is_none());
        assert!(resolved.password.is_none());
    }
}
