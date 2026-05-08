#![allow(dead_code)]

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ControlCommand {
    Pause,
    Resume,
    CleanNow,
    Restore,
    OpenConfig,
    Diagnostics,
    Quit,
    GetStatus,
    SubscribeStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum BackendMode {
    X11,
    Wayland,
    Limited,
    None,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum BackendCapability {
    Automatic,
    Limited,
    DiagnosticsOnly,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct StatusSnapshot {
    pub paused: bool,
    pub backend: BackendMode,
    pub capability: BackendCapability,
    pub last_clean_succeeded: bool,
    pub clean_now_available: bool,
    pub restore_available: bool,
    pub show_success_flash: bool,
}

impl StatusSnapshot {
    pub fn new(paused: bool, backend: BackendMode, capability: BackendCapability) -> Self {
        Self {
            paused,
            backend,
            capability,
            last_clean_succeeded: true,
            clean_now_available: !matches!(capability, BackendCapability::DiagnosticsOnly),
            restore_available: false,
            show_success_flash: false,
        }
    }

    pub fn can_clean_now(&self) -> bool {
        self.clean_now_available
    }
}

impl ControlCommand {
    pub fn encode_line(self) -> &'static str {
        match self {
            ControlCommand::Pause => "pause",
            ControlCommand::Resume => "resume",
            ControlCommand::CleanNow => "clean-now",
            ControlCommand::Restore => "restore",
            ControlCommand::OpenConfig => "open-config",
            ControlCommand::Diagnostics => "diagnostics",
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
            "restore" => Some(ControlCommand::Restore),
            "open-config" => Some(ControlCommand::OpenConfig),
            "diagnostics" => Some(ControlCommand::Diagnostics),
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
            "paused={};backend={};capability={};last_clean_succeeded={};clean_now_available={};restore_available={};show_success_flash={}",
            self.paused,
            self.backend.as_str(),
            self.capability.as_str(),
            self.last_clean_succeeded,
            self.clean_now_available,
            self.restore_available,
            self.show_success_flash
        )
    }

    pub fn decode_line(value: &str) -> Option<Self> {
        let mut paused = None;
        let mut backend = None;
        let mut capability = None;
        let mut last_clean_succeeded = None;
        let mut clean_now_available = None;
        let mut restore_available = None;
        let mut show_success_flash = None;

        for part in value.split(';') {
            let (key, value) = part.split_once('=')?;
            match key {
                "paused" => paused = Some(matches!(value, "true")),
                "backend" => backend = BackendMode::parse(value),
                "capability" => capability = BackendCapability::parse(value),
                "last_clean_succeeded" => last_clean_succeeded = Some(matches!(value, "true")),
                "clean_now_available" => clean_now_available = Some(matches!(value, "true")),
                "restore_available" => restore_available = Some(matches!(value, "true")),
                "show_success_flash" => show_success_flash = Some(matches!(value, "true")),
                _ => {} // ignore unknown keys for forward compatibility
            }
        }

        let capability = capability?;

        Some(Self {
            paused: paused?,
            backend: backend?,
            capability,
            last_clean_succeeded: last_clean_succeeded?,
            clean_now_available: clean_now_available
                .unwrap_or(!matches!(capability, BackendCapability::DiagnosticsOnly)),
            restore_available: restore_available.unwrap_or(false),
            show_success_flash: show_success_flash.unwrap_or(false),
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
            clean_now_available: false,
            restore_available: false,
            show_success_flash: false,
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
            clean_now_available: false,
            restore_available: false,
            show_success_flash: true,
        };

        assert_eq!(
            StatusSnapshot::decode_line(&snapshot.encode_line()),
            Some(snapshot)
        );
    }

    #[test]
    fn test_status_snapshot_decode_defaults_clean_now_for_older_payloads() {
        let snapshot = StatusSnapshot::decode_line(
            "paused=false;backend=Limited;capability=Limited;last_clean_succeeded=true",
        )
        .unwrap();

        assert!(snapshot.clean_now_available);
        assert!(!snapshot.restore_available);
    }

    #[test]
    fn test_new_control_commands_round_trip() {
        for command in [
            ControlCommand::Restore,
            ControlCommand::OpenConfig,
            ControlCommand::Diagnostics,
        ] {
            assert_eq!(
                ControlCommand::decode_line(command.encode_line()),
                Some(command)
            );
        }
    }
}
