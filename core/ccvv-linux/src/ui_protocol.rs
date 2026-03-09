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

impl StatusSnapshot {
    pub fn encode_line(&self) -> String {
        format!(
            "paused={};backend={:?};capability={:?};last_clean_succeeded={}",
            self.paused, self.backend, self.capability, self.last_clean_succeeded
        )
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
}
