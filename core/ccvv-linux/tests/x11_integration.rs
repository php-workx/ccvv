mod common;

use ccvv_linux::backend::x11::X11Backend;
use ccvv_linux::backend::ClipboardBackend;

#[test]
fn x11_backend_source_name() {
    let backend = X11Backend::new();
    assert_eq!(backend.source_name(), "x11");
}

#[test]
fn x11_backend_capability_is_automatic() {
    let backend = X11Backend::new();
    assert_eq!(
        backend.capability(),
        ccvv_linux::ui_protocol::BackendCapability::Automatic
    );
}

#[test]
fn x11_backend_read_without_display_returns_error() {
    // On CI/macOS without X11, this should return Unavailable or Protocol error
    let mut backend = X11Backend::new();
    let result = backend.read_snapshot();
    assert!(result.is_err(), "expected error without X11 display");
}

#[test]
fn x11_backend_write_without_display_returns_error() {
    let mut backend = X11Backend::new();
    let result = backend.write_plain_text("ccvv-x11-write-without-display");
    assert!(result.is_err(), "expected write error without X11 display");
}

#[test]
fn x11_backend_subscribe_without_display_returns_error() {
    let mut backend = X11Backend::new();
    let result = backend.subscribe();
    assert!(
        result.is_err(),
        "expected subscribe error without X11 display"
    );
}

#[test]
#[ignore = "requires live X11 display with clipboard access"]
fn x11_read_write_round_trip() {
    if !common::has_x11_display() {
        return;
    }
    let mut backend = X11Backend::new();
    let text = "ccvv-x11-integration-test-payload";
    let token = backend
        .write_plain_text(text)
        .expect("write should succeed on live X11");
    assert!(token.backend_serial.is_some());

    let snapshot = backend
        .read_snapshot()
        .expect("read should succeed after write");
    assert_eq!(snapshot.acquired_plain_text, text);
}
