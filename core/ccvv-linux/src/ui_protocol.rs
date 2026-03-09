#![allow(dead_code)]

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlCommand {
    Pause,
    Resume,
    CleanNow,
    Quit,
    GetStatus,
    SubscribeStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendMode {
    X11,
    Wayland,
    Limited,
    None,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackendCapability {
    Automatic,
    Limited,
    DiagnosticsOnly,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatusSnapshot {
    pub paused: bool,
    pub backend: BackendMode,
    pub capability: BackendCapability,
    pub last_clean_succeeded: bool,
}

impl ControlCommand {
    pub fn encode_line(self) -> &'static str {
        match self {
            ControlCommand::Pause => "pause",
            ControlCommand::Resume => "resume",
            ControlCommand::CleanNow => "clean-now",
            ControlCommand::Quit => "quit",
            ControlCommand::GetStatus => "get-status",
            ControlCommand::SubscribeStatus => "subscribe-status",
        }
    }

    pub fn decode_line(value: &str) -> Option<Self> {
        match value {
            "pause" => Some(ControlCommand::Pause),
            "resume" => Some(ControlCommand::Resume),
            "clean-now" => Some(ControlCommand::CleanNow),
            "quit" => Some(ControlCommand::Quit),
            "get-status" => Some(ControlCommand::GetStatus),
            "subscribe-status" => Some(ControlCommand::SubscribeStatus),
            _ => None,
        }
    }
}

impl BackendMode {
    pub fn as_str(self) -> &'static str {
        match self {
            BackendMode::X11 => "X11",
            BackendMode::Wayland => "Wayland",
            BackendMode::Limited => "Limited",
            BackendMode::None => "None",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "X11" => Some(BackendMode::X11),
            "Wayland" => Some(BackendMode::Wayland),
            "Limited" => Some(BackendMode::Limited),
            "None" => Some(BackendMode::None),
            _ => None,
        }
    }
}

impl BackendCapability {
    pub fn as_str(self) -> &'static str {
        match self {
            BackendCapability::Automatic => "Automatic",
            BackendCapability::Limited => "Limited",
            BackendCapability::DiagnosticsOnly => "DiagnosticsOnly",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "Automatic" => Some(BackendCapability::Automatic),
            "Limited" => Some(BackendCapability::Limited),
            "DiagnosticsOnly" => Some(BackendCapability::DiagnosticsOnly),
            _ => None,
        }
    }
}

impl StatusSnapshot {
    pub fn encode_line(&self) -> String {
        format!(
            "paused={};backend={};capability={};last_clean_succeeded={}",
            self.paused,
            self.backend.as_str(),
            self.capability.as_str(),
            self.last_clean_succeeded
        )
    }

    pub fn decode_line(value: &str) -> Option<Self> {
        let mut paused = None;
        let mut backend = None;
        let mut capability = None;
        let mut last_clean_succeeded = None;

        for part in value.split(';') {
            let (key, value) = part.split_once('=')?;
            match key {
                "paused" => paused = Some(matches!(value, "true")),
                "backend" => backend = BackendMode::parse(value),
                "capability" => capability = BackendCapability::parse(value),
                "last_clean_succeeded" => last_clean_succeeded = Some(matches!(value, "true")),
                _ => {} // ignore unknown keys for forward compatibility
            }
        }

        Some(Self {
            paused: paused?,
            backend: backend?,
            capability: capability?,
            last_clean_succeeded: last_clean_succeeded?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{BackendCapability, BackendMode, ControlCommand, StatusSnapshot};

    #[test]
    fn test_control_command_round_trip() {
        let command = ControlCommand::SubscribeStatus;

        assert_eq!(
            ControlCommand::decode_line(command.encode_line()),
            Some(command)
        );
    }

    #[test]
    fn test_status_snapshot_encodes_without_clipboard_payload() {
        let snapshot = StatusSnapshot {
            paused: false,
            backend: BackendMode::None,
            capability: BackendCapability::DiagnosticsOnly,
            last_clean_succeeded: true,
        };

        let encoded = snapshot.encode_line();
        assert!(encoded.contains("paused=false"));
        assert!(!encoded.contains("clipboard"));
    }

    #[test]
    fn test_status_snapshot_round_trip() {
        let snapshot = StatusSnapshot {
            paused: true,
            backend: BackendMode::Limited,
            capability: BackendCapability::Limited,
            last_clean_succeeded: false,
        };

        assert_eq!(
            StatusSnapshot::decode_line(&snapshot.encode_line()),
            Some(snapshot)
        );
    }
}
