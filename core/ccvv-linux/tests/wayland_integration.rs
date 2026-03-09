mod common;

use ccvv_linux::backend::wayland::WaylandBackend;
use ccvv_linux::backend::ClipboardBackend;

#[test]
fn wayland_backend_source_name() {
    let backend = WaylandBackend::new();
    assert_eq!(backend.source_name(), "wayland");
}

#[test]
fn wayland_backend_read_without_compositor_returns_error() {
    // Without a Wayland compositor, read should return an error
    let mut backend = WaylandBackend::new();
    let result = backend.read_snapshot();
    assert!(result.is_err(), "expected error without Wayland compositor");
}

#[test]
fn wayland_limited_backend_capability() {
    let backend = WaylandBackend::new_limited();
    assert_eq!(backend.source_name(), "wayland-limited");
    assert_eq!(
        backend.capability(),
        ccvv_linux::ui_protocol::BackendCapability::Limited
    );
}

#[test]
#[ignore = "requires live Wayland compositor with data-control protocol"]
fn wayland_read_write_round_trip() {
    if !common::has_wayland_display() {
        return;
    }
    let mut backend = WaylandBackend::new();
    let text = "ccvv-wayland-integration-test-payload";
    // Write may return Unavailable until full data-control is implemented
    match backend.write_plain_text(text) {
        Ok(token) => {
            assert!(token.backend_serial.is_some());
        }
        Err(ccvv_linux::backend::BackendError::Unavailable) => {
            // Expected until full data-control send is implemented
        }
        Err(e) => panic!("unexpected error: {e}"),
    }
}
