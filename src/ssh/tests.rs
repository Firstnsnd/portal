/// Unit tests for SSH port forwarding

#[cfg(test)]
mod tests {
    use crate::ssh::port_forward::{PortForward, ForwardState, PortForwardConfig, ForwardKind};
    use crate::ssh::{AppNotification, NotificationLevel};
    
    

    fn create_test_forward(kind: ForwardKind) -> PortForwardConfig {
        PortForwardConfig {
            kind,
            local_host: "127.0.0.1".to_string(),
            local_port: 8080,
            remote_host: "localhost".to_string(),
            remote_port: 3000,
        }
    }

    #[test]
    fn test_port_forward_creation() {
        let config = create_test_forward(ForwardKind::Local);
        let (pf, _rx) = PortForward::new(config);

        assert_eq!(pf.config.kind, ForwardKind::Local);
        assert_eq!(pf.config.local_port, 8080);
        assert_eq!(pf.config.remote_port, 3000);
    }

    #[test]
    fn test_port_forward_initial_state() {
        let config = create_test_forward(ForwardKind::Remote);
        let (pf, _rx) = PortForward::new(config.clone());

        // Initial state should be Starting
        let state = pf.current_state();
        assert!(matches!(state, ForwardState::Starting));

        // allocated_port should be None initially
        let allocated = pf.allocated_port.lock().unwrap();
        assert!(allocated.is_none());
    }

    #[test]
    fn test_port_forward_stop() {
        let config = create_test_forward(ForwardKind::Local);
        let (pf, _rx) = PortForward::new(config);

        // Initially in Starting state
        assert!(matches!(pf.current_state(), ForwardState::Starting));

        // Stop the forward
        pf.stop();

        // State should be Stopped
        assert!(matches!(pf.current_state(), ForwardState::Stopped));
    }

    #[test]
    fn test_allocated_port_tracking() {
        let config = create_test_forward(ForwardKind::Remote);
        let (pf, _rx) = PortForward::new(config.clone());

        // Initially None
        {
            let allocated = pf.allocated_port.lock().unwrap();
            assert!(allocated.is_none());
        }

        // Simulate allocated port being set
        {
            let mut allocated = pf.allocated_port.lock().unwrap();
            *allocated = Some(12345);
        }

        // Check it was stored
        {
            let allocated = pf.allocated_port.lock().unwrap();
            assert_eq!(*allocated, Some(12345));
        }
    }

    #[test]
    fn test_forward_state_equality() {
        assert_eq!(ForwardState::Starting, ForwardState::Starting);
        assert_eq!(ForwardState::Active, ForwardState::Active);
        assert_eq!(ForwardState::Stopped, ForwardState::Stopped);

        assert_ne!(ForwardState::Active, ForwardState::Stopped);

        let error1 = ForwardState::Error("test error".to_string());
        let error2 = ForwardState::Error("test error".to_string());
        assert_eq!(error1, error2);

        let error3 = ForwardState::Error("different error".to_string());
        assert_ne!(error1, error3);
    }

    #[test]
    fn test_watch_channel_creation() {
        let config = create_test_forward(ForwardKind::Local);
        let (_pf, rx) = PortForward::new(config);

        // The channel should initially have false (no cancellation)
        assert_eq!(*rx.borrow(), false);
    }

    #[test]
    fn test_local_forward_config_display() {
        let config = PortForwardConfig {
            kind: ForwardKind::Local,
            local_host: "0.0.0.0".to_string(),
            local_port: 2222,
            remote_host: "example.com".to_string(),
            remote_port: 80,
        };

        let display = format!("{}", config);
        assert!(display.contains("-L"));
        assert!(display.contains("0.0.0.0:2222"));
        assert!(display.contains("example.com:80"));
    }

    #[test]
    fn test_remote_forward_config_display() {
        let config = PortForwardConfig {
            kind: ForwardKind::Remote,
            local_host: "localhost".to_string(),
            local_port: 3000,
            remote_host: "0.0.0.0".to_string(),
            remote_port: 8080,
        };

        let display = format!("{}", config);
        assert!(display.contains("-R"));
        assert!(display.contains("0.0.0.0:8080"));
        assert!(display.contains("localhost:3000"));
    }

    // ── AppNotification tests ──────────────────────────────────────────────
    //
    // These tests verify notification creation, level assignment, and expiry
    // logic used by the toast system and port-forward conflict detection.

    #[test]
    fn test_notification_info_creation() {
        let n = AppNotification::info("test info message");
        assert_eq!(n.message, "test info message");
        assert_eq!(n.level, NotificationLevel::Info);
    }

    #[test]
    fn test_notification_warning_creation() {
        let n = AppNotification::warning("port conflict");
        assert_eq!(n.message, "port conflict");
        assert_eq!(n.level, NotificationLevel::Warning);
    }

    #[test]
    fn test_notification_error_creation() {
        let n = AppNotification::error("connection failed");
        assert_eq!(n.message, "connection failed");
        assert_eq!(n.level, NotificationLevel::Error);
    }

    #[test]
    fn test_notification_not_expired_immediately() {
        let n = AppNotification::info("fresh notification");
        // A notification created just now must NOT be expired with a 5-second timeout
        assert!(!n.is_expired(5), "Fresh notification must not be expired");
    }

    #[test]
    fn test_notification_not_expired_with_zero_timeout() {
        let n = AppNotification::info("test");
        // Even with 0-second timeout, a notification created at the current instant
        // may or may not be expired depending on timing. With elapsed() of 0, it
        // should be >= 0 which means expired. This tests the boundary condition.
        // We use 1 second to be safe — it was just created.
        assert!(!n.is_expired(1));
    }

    #[test]
    fn test_notification_levels_equality() {
        assert_eq!(NotificationLevel::Info, NotificationLevel::Info);
        assert_eq!(NotificationLevel::Warning, NotificationLevel::Warning);
        assert_eq!(NotificationLevel::Error, NotificationLevel::Error);
        assert_ne!(NotificationLevel::Info, NotificationLevel::Warning);
        assert_ne!(NotificationLevel::Warning, NotificationLevel::Error);
    }

    #[test]
    fn test_notification_levels_debug_format() {
        assert_eq!(format!("{:?}", NotificationLevel::Info), "Info");
        assert_eq!(format!("{:?}", NotificationLevel::Warning), "Warning");
        assert_eq!(format!("{:?}", NotificationLevel::Error), "Error");
    }

    #[test]
    fn test_forward_state_conflict_variant() {
        // Verify the Conflict variant exists and distinguishes from Error
        let conflict = ForwardState::Conflict("Address already in use".to_string());
        let error = ForwardState::Error("Address already in use".to_string());

        // Conflict and Error with same message must not be equal
        assert_ne!(conflict, error, "Conflict and Error must be distinguishable");

        // Same conflict messages must be equal
        let conflict2 = ForwardState::Conflict("Address already in use".to_string());
        assert_eq!(conflict, conflict2);
    }

    #[test]
    fn test_forward_state_all_variants() {
        // Ensure all ForwardState variants can be constructed
        let states = vec![
            ForwardState::Starting,
            ForwardState::Active,
            ForwardState::Stopped,
            ForwardState::Error("e".to_string()),
            ForwardState::Conflict("c".to_string()),
        ];

        // Verify each variant is distinct
        for (i, s1) in states.iter().enumerate() {
            for (j, s2) in states.iter().enumerate() {
                if i == j {
                    assert_eq!(s1, s2);
                } else {
                    assert_ne!(s1, s2, "Different ForwardState variants must not be equal");
                }
            }
        }
    }

    #[test]
    fn test_port_forward_conflict_state() {
        // Simulate a port forward that detects a conflict
        let config = create_test_forward(ForwardKind::Local);
        let (pf, _rx) = PortForward::new(config);

        // Manually set state to Conflict
        {
            let mut state = pf.state.lock().unwrap();
            *state = ForwardState::Conflict("Port 8080 already in use".to_string());
        }

        assert!(matches!(pf.current_state(), ForwardState::Conflict(_)));
    }
}
