use std::collections::HashMap;

use sha2::{Digest, Sha256};

use ccvv_lib::AdaptiveTimingWindow;

use crate::backend::{ClipboardSnapshot, SelectionKind};

// Spec §7.2: Linux timing window default 400ms, configurable in [150, 600].
// The clamp matters even when adaptive mode is enabled — if a user has only
// ever copied very fast or very slow, we still keep the active window inside
// the spec's human-tap range.
const DEFAULT_TRIGGER_WINDOW_MS: u64 = 400;
pub(crate) const MIN_TRIGGER_WINDOW_MS: u64 = 150;
pub(crate) const MAX_TRIGGER_WINDOW_MS: u64 = 600;

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
    pub last_source_kind: Option<SelectionKind>,
}

#[derive(Clone, Debug, Default)]
pub struct DetectorState {
    seats: HashMap<String, SeatState>,
    paused: bool,
    /// When `Some`, a fixed double-tap window from config takes precedence
    /// over per-seat adaptive timing (spec §7.2 "Config value takes
    /// precedence (disables adaptive if fixed)").
    fixed_window_ms: Option<u64>,
}

impl DetectorState {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct a detector that honors a fixed double-tap window from config.
    /// Pass `None` to enable adaptive timing. The value is clamped to the
    /// Linux fixed-window range.
    pub fn with_fixed_window_ms(window_ms: Option<u64>) -> Self {
        Self {
            fixed_window_ms: window_ms
                .map(|ms| ms.clamp(MIN_TRIGGER_WINDOW_MS, MAX_TRIGGER_WINDOW_MS)),
            ..Self::default()
        }
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
                let active_window = self
                    .fixed_window_ms
                    .or_else(|| seat_state.adaptive_timing.threshold_ms().map(u64::from))
                    .unwrap_or(DEFAULT_TRIGGER_WINDOW_MS)
                    .clamp(MIN_TRIGGER_WINDOW_MS, MAX_TRIGGER_WINDOW_MS);
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
        seat_state.last_source_kind = Some(snapshot.selection_kind.clone());
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
    fn test_fixed_window_from_config_takes_precedence_over_adaptive() {
        // Even after many fast samples (which would push adaptive threshold
        // very low), a fixed window from config governs the active window.
        let mut state = DetectorState::with_fixed_window_ms(Some(450));
        for _ in 0..20 {
            state
                .seats
                .entry("seat0".to_string())
                .or_default()
                .adaptive_timing
                .record_sample(50);
        }

        assert_eq!(
            state.observe(&snapshot("hello", 100, false, SelectionKind::Clipboard)),
            DetectionOutcome::TrackedFirstCopy
        );
        // Interval 400ms should still trigger because fixed window is 450.
        assert_eq!(
            state.observe(&snapshot("hello", 500, false, SelectionKind::Clipboard)),
            DetectionOutcome::TriggeredClean
        );
    }

    #[test]
    fn test_fixed_window_clamped_to_linux_range() {
        // Ridiculously high config value gets clamped to MAX (600ms);
        // a 700ms gap should NOT trigger.
        let mut state = DetectorState::with_fixed_window_ms(Some(2_000));

        assert_eq!(
            state.observe(&snapshot("hello", 100, false, SelectionKind::Clipboard)),
            DetectionOutcome::TrackedFirstCopy
        );
        assert_eq!(
            state.observe(&snapshot("hello", 800, false, SelectionKind::Clipboard)),
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

    #[test]
    fn test_self_write_does_not_arm_follow_up_trigger() {
        let mut state = DetectorState::new();

        assert_eq!(
            state.observe(&snapshot("hello", 100, true, SelectionKind::Clipboard)),
            DetectionOutcome::IgnoredSelfWrite
        );
        assert_eq!(
            state.observe(&snapshot("hello", 150, false, SelectionKind::Clipboard)),
            DetectionOutcome::TrackedFirstCopy
        );
    }

    #[test]
    fn test_triggered_clean_resets_same_text_to_first_copy() {
        let mut state = DetectorState::new();

        assert_eq!(
            state.observe(&snapshot("hello", 100, false, SelectionKind::Clipboard)),
            DetectionOutcome::TrackedFirstCopy
        );
        assert_eq!(
            state.observe(&snapshot("hello", 200, false, SelectionKind::Clipboard)),
            DetectionOutcome::TriggeredClean
        );
        assert_eq!(
            state.observe(&snapshot("hello", 260, false, SelectionKind::Clipboard)),
            DetectionOutcome::TrackedFirstCopy
        );
    }

    #[test]
    fn test_different_text_starts_new_candidate_chain() {
        let mut state = DetectorState::new();

        assert_eq!(
            state.observe(&snapshot("hello", 100, false, SelectionKind::Clipboard)),
            DetectionOutcome::TrackedFirstCopy
        );
        assert_eq!(
            state.observe(&snapshot("world", 180, false, SelectionKind::Clipboard)),
            DetectionOutcome::TrackedFirstCopy
        );
        assert_eq!(
            state.observe(&snapshot("world", 260, false, SelectionKind::Clipboard)),
            DetectionOutcome::TriggeredClean
        );
    }
}
