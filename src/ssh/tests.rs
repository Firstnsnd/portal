/// Unit tests for SSH port forwarding

#[cfg(test)]
mod tests {
    use crate::ssh::port_forward::{PortForward, ForwardState, PortForwardConfig, ForwardKind};
    use std::sync::Arc;
    use tokio::sync::watch;

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
}
