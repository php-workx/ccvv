#![allow(dead_code)]

use std::env;

pub fn has_x11_display() -> bool {
    env::var("DISPLAY").is_ok()
}

pub fn has_wayland_display() -> bool {
    env::var("WAYLAND_DISPLAY").is_ok()
}

pub fn skip_message(feature: &str) -> String {
    format!("skipped {feature} integration (environment not available)")
}
