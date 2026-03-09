//! Adaptive timing helpers shared across FFI and native Rust callers.

use std::sync::Mutex;

const MIN_SAMPLES: usize = 10;
const MAX_SAMPLES: usize = 100;
const PERCENTILE_NUMERATOR: usize = 90;

#[derive(Debug, Clone, Default)]
pub struct AdaptiveTimingWindow {
    samples: Vec<u32>,
}

impl AdaptiveTimingWindow {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_sample(&mut self, interval_ms: u32) {
        self.samples.push(interval_ms);
        if self.samples.len() > MAX_SAMPLES {
            let excess = self.samples.len() - MAX_SAMPLES;
            self.samples.drain(..excess);
        }
    }

    pub fn threshold_ms(&self) -> Option<u32> {
        if self.samples.len() < MIN_SAMPLES {
            return None;
        }

        let mut sorted = self.samples.clone();
        sorted.sort_unstable();
        let index = (sorted.len() * PERCENTILE_NUMERATOR) / 100;
        sorted.get(index).copied()
    }
}

static GLOBAL_WINDOW: Mutex<AdaptiveTimingWindow> = Mutex::new(AdaptiveTimingWindow {
    samples: Vec::new(),
});

pub fn record_global_sample(interval_ms: u32) {
    let mut window = GLOBAL_WINDOW
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    window.record_sample(interval_ms);
}

pub fn global_threshold_ms() -> Option<u32> {
    let window = GLOBAL_WINDOW
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    window.threshold_ms()
}

#[cfg(test)]
mod tests {
    use super::AdaptiveTimingWindow;

    #[test]
    fn test_threshold_requires_enough_samples() {
        let mut window = AdaptiveTimingWindow::new();
        for value in 100..109 {
            window.record_sample(value);
        }

        assert_eq!(window.threshold_ms(), None);
    }

    #[test]
    fn test_threshold_uses_recent_capped_percentile() {
        let mut window = AdaptiveTimingWindow::new();
        for value in 1..=120 {
            window.record_sample(value);
        }

        assert_eq!(window.threshold_ms(), Some(111));
    }
}
