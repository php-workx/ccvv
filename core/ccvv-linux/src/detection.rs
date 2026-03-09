use sha2::{Digest, Sha256};

use ccvv_lib::AdaptiveTimingWindow;

use crate::backend::ClipboardSnapshot;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DetectionOutcome {
    IgnoredSelfWrite,
    IgnoredWhilePaused,
    TrackedFirstCopy,
    TriggeredClean,
}

#[derive(Clone, Debug)]
pub struct DetectorState {
    pub last_hash: Option<[u8; 32]>,
    pub last_timestamp: Option<u64>,
    pub last_backend_serial: Option<u64>,
    pub adaptive_timing: AdaptiveTimingWindow,
    paused: bool,
}

impl Default for DetectorState {
    fn default() -> Self {
        Self {
            last_hash: None,
            last_timestamp: None,
            last_backend_serial: None,
            adaptive_timing: AdaptiveTimingWindow::new(),
            paused: false,
        }
    }
}

impl DetectorState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn pause(&mut self) {
        self.paused = true;
    }

    pub fn resume(&mut self) {
        self.paused = false;
    }

    pub fn observe(&mut self, snapshot: &ClipboardSnapshot) -> DetectionOutcome {
        if self.paused {
            return DetectionOutcome::IgnoredWhilePaused;
        }
        if snapshot.is_self_write {
            return DetectionOutcome::IgnoredSelfWrite;
        }

        let current_hash = digest(snapshot.acquired_plain_text.as_bytes());

        if let (Some(previous_hash), Some(previous_timestamp)) =
            (self.last_hash, self.last_timestamp)
        {
            if previous_hash == current_hash {
                let interval = snapshot.timestamp.saturating_sub(previous_timestamp);
                self.adaptive_timing.record_sample(interval as u32);
                self.last_timestamp = Some(snapshot.timestamp);
                self.last_backend_serial = snapshot.backend_serial;
                return DetectionOutcome::TriggeredClean;
            }
        }

        self.last_hash = Some(current_hash);
        self.last_timestamp = Some(snapshot.timestamp);
        self.last_backend_serial = snapshot.backend_serial;
        DetectionOutcome::TrackedFirstCopy
    }
}

fn digest(input: &[u8]) -> [u8; 32] {
    let mut output = [0_u8; 32];
    output.copy_from_slice(&Sha256::digest(input));
    output
}

#[cfg(test)]
mod tests {
    use crate::backend::{ClipboardSnapshot, SelectionKind};
    use crate::detection::{DetectionOutcome, DetectorState};

    fn snapshot(text: &str, timestamp: u64, is_self_write: bool) -> ClipboardSnapshot {
        ClipboardSnapshot {
            seat_id: "seat0".to_string(),
            selection_kind: SelectionKind::Clipboard,
            acquired_plain_text: text.to_string(),
            acquired_html: None,
            timestamp,
            backend_serial: Some(timestamp),
            is_self_write,
        }
    }

    #[test]
    fn test_repeated_copy_triggers_clean() {
        let mut state = DetectorState::new();

        assert_eq!(
            state.observe(&snapshot("hello", 100, false)),
            DetectionOutcome::TrackedFirstCopy
        );
        assert_eq!(
            state.observe(&snapshot("hello", 320, false)),
            DetectionOutcome::TriggeredClean
        );
    }

    #[test]
    fn test_self_write_is_ignored() {
        let mut state = DetectorState::new();

        assert_eq!(
            state.observe(&snapshot("hello", 100, true)),
            DetectionOutcome::IgnoredSelfWrite
        );
    }

    #[test]
    fn test_pause_and_resume() {
        let mut state = DetectorState::new();
        state.pause();
        assert_eq!(
            state.observe(&snapshot("hello", 100, false)),
            DetectionOutcome::IgnoredWhilePaused
        );

        state.resume();
        assert_eq!(
            state.observe(&snapshot("hello", 120, false)),
            DetectionOutcome::TrackedFirstCopy
        );
    }
}
