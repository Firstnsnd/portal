//! Per-host capability probe results, shared by the SSH session and the UI.
//!
//! Platform-neutral on purpose: SFTP matters on every platform, so the
//! tri-state can't live in the cfg(unix) zmodem module (whose
//! `ZmodemSupport` is an alias of [`HostCapability`]).

/// Tri-state support flag for a host capability probed after connect
/// (SFTP subsystem, ZMODEM sz/rz).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostCapability {
    /// Not probed yet, or the probe itself failed (e.g. a hardened server
    /// disables exec, or the probe timed out). Never gate UI on this —
    /// let the feature attempt and report a precise error instead.
    Unknown,
    /// The probe confirmed the capability is present.
    Available,
    /// The probe ran and confirmed the capability is absent.
    Missing,
}

impl HostCapability {
    pub fn to_u8(self) -> u8 {
        self as u8
    }

    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Available,
            2 => Self::Missing,
            _ => Self::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u8_roundtrip() {
        for v in [
            HostCapability::Unknown,
            HostCapability::Available,
            HostCapability::Missing,
        ] {
            assert_eq!(HostCapability::from_u8(v.to_u8()), v);
        }
        // Out-of-range values degrade to Unknown, never to a wrong answer.
        assert_eq!(HostCapability::from_u8(99), HostCapability::Unknown);
    }
}
