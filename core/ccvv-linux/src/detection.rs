use std::collections::HashMap;

use sha2::{Digest, Sha256};

use ccvv_lib::AdaptiveTimingWindow;

use crate::backend::{ClipboardSnapshot, SelectionKind};

const DEFAULT_TRIGGER_WINDOW_MS: u64 = 400;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DetectionOutcome {
    IgnoredNonClipboardSelection,
    IgnoredSelfWrite,
    IgnoredWhilePaused,
    TrackedFirstCopy,
    TriggeredClean,
}

#[derive(Clone, Debug, Default)]
struct SeatState {
    pub last_hash: Option<[u8; 32]>,
    pub last_timestamp: Option<u64>,
    pub last_backend_serial: Option<u64>,
    pub adaptive_timing: AdaptiveTimingWindow,
}

#[derive(Clone, Debug, Default)]
pub struct DetectorState {
    seats: HashMap<String, SeatState>,
    paused: bool,
}

impl DetectorState {
    pub fn new() -> Self {
        Self::default()
    }

    #[allow(dead_code)]
    pub fn pause(&mut self) {
        self.paused = true;
    }

    #[allow(dead_code)]
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
        if snapshot.selection_kind != SelectionKind::Clipboard {
            return DetectionOutcome::IgnoredNonClipboardSelection;
        }

        let seat_state = self.seats.entry(snapshot.seat_id.clone()).or_default();
        let current_hash = digest(snapshot.acquired_plain_text.as_bytes());
        let outcome = DetectionOutcome::TrackedFirstCopy;

        if let (Some(previous_hash), Some(previous_timestamp)) =
            (seat_state.last_hash, seat_state.last_timestamp)
        {
            if previous_hash == current_hash {
                let interval = snapshot.timestamp.saturating_sub(previous_timestamp);
                let active_window = seat_state
                    .adaptive_timing
                    .threshold_ms()
                    .map(u64::from)
                    .unwrap_or(DEFAULT_TRIGGER_WINDOW_MS);
                if interval <= active_window {
                    let clamped = u32::try_from(interval).unwrap_or(u32::MAX);
                    seat_state.adaptive_timing.record_sample(clamped);
                    // Reset state so the next copy of the same text is treated as first-copy
                    seat_state.last_hash = None;
                    seat_state.last_timestamp = None;
                    seat_state.last_backend_serial = None;
                    return DetectionOutcome::TriggeredClean;
                }
            }
        }

        seat_state.last_hash = Some(current_hash);
        seat_state.last_timestamp = Some(snapshot.timestamp);
        seat_state.last_backend_serial = snapshot.backend_serial;
        outcome
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

    fn snapshot(
        text: &str,
        timestamp: u64,
        is_self_write: bool,
        selection_kind: SelectionKind,
    ) -> ClipboardSnapshot {
        ClipboardSnapshot {
            seat_id: "seat0".to_string(),
            selection_kind,
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
            state.observe(&snapshot("hello", 100, false, SelectionKind::Clipboard)),
            DetectionOutcome::TrackedFirstCopy
        );
        assert_eq!(
            state.observe(&snapshot("hello", 320, false, SelectionKind::Clipboard)),
            DetectionOutcome::TriggeredClean
        );
    }

    #[test]
    fn test_self_write_is_ignored() {
        let mut state = DetectorState::new();

        assert_eq!(
            state.observe(&snapshot("hello", 100, true, SelectionKind::Clipboard)),
            DetectionOutcome::IgnoredSelfWrite
        );
    }

    #[test]
    fn test_pause_and_resume() {
        let mut state = DetectorState::new();
        state.pause();
        assert_eq!(
            state.observe(&snapshot("hello", 100, false, SelectionKind::Clipboard)),
            DetectionOutcome::IgnoredWhilePaused
        );

        state.resume();
        assert_eq!(
            state.observe(&snapshot("hello", 120, false, SelectionKind::Clipboard)),
            DetectionOutcome::TrackedFirstCopy
        );
    }

    #[test]
    fn test_repeated_copy_outside_window_is_tracked_as_first_copy() {
        let mut state = DetectorState::new();
        for _ in 0..10 {
            state
                .seats
                .entry("seat0".to_string())
                .or_default()
                .adaptive_timing
                .record_sample(100);
        }

        assert_eq!(
            state.observe(&snapshot("hello", 100, false, SelectionKind::Clipboard)),
            DetectionOutcome::TrackedFirstCopy
        );
        assert_eq!(
            state.observe(&snapshot("hello", 450, false, SelectionKind::Clipboard)),
            DetectionOutcome::TrackedFirstCopy
        );
    }

    #[test]
    fn test_separate_seats_do_not_share_timing_state() {
        let mut state = DetectorState::new();

        assert_eq!(
            state.observe(&snapshot_for_seat(
                "hello",
                100,
                "seat0",
                false,
                SelectionKind::Clipboard
            )),
            DetectionOutcome::TrackedFirstCopy
        );
        assert_eq!(
            state.observe(&snapshot_for_seat(
                "hello",
                200,
                "seat1",
                false,
                SelectionKind::Clipboard
            )),
            DetectionOutcome::TrackedFirstCopy
        );
        assert_eq!(
            state.observe(&snapshot_for_seat(
                "hello",
                320,
                "seat0",
                false,
                SelectionKind::Clipboard
            )),
            DetectionOutcome::TriggeredClean
        );
        assert_eq!(
            state.observe(&snapshot_for_seat(
                "hello",
                700,
                "seat1",
                false,
                SelectionKind::Clipboard
            )),
            DetectionOutcome::TrackedFirstCopy
        );
    }

    fn snapshot_for_seat(
        text: &str,
        timestamp: u64,
        seat: &str,
        is_self_write: bool,
        selection_kind: SelectionKind,
    ) -> ClipboardSnapshot {
        ClipboardSnapshot {
            seat_id: seat.to_string(),
            selection_kind,
            acquired_plain_text: text.to_string(),
            acquired_html: None,
            timestamp,
            backend_serial: Some(timestamp),
            is_self_write,
        }
    }

    #[test]
    fn test_primary_does_not_create_clipboard_double_copy_candidate() {
        let mut state = DetectorState::new();

        assert_eq!(
            state.observe(&snapshot("hello", 100, false, SelectionKind::Primary)),
            DetectionOutcome::IgnoredNonClipboardSelection
        );
        assert_eq!(
            state.observe(&snapshot("hello", 120, false, SelectionKind::Clipboard)),
            DetectionOutcome::TrackedFirstCopy
        );
    }
}
