mod common;

use ccvv_linux::backend::wayland::{WaylandBackend, WaylandSupport};
use ccvv_linux::backend::ClipboardBackend;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static HARNESS_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

fn harness_lock() -> MutexGuard<'static, ()> {
    HARNESS_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn wayland_backend_source_name() {
    let backend = WaylandBackend::new();
    assert_eq!(backend.source_name(), "wayland");
}

#[test]
fn wayland_backend_read_without_compositor_returns_error() {
    if common::has_wayland_display() {
        return;
    }

    let mut backend = WaylandBackend::new();
    let result = backend.read_snapshot();
    assert!(result.is_err(), "expected error without Wayland compositor");
}

#[test]
fn wayland_backend_subscribe_without_compositor_returns_error() {
    if common::has_wayland_display() {
        return;
    }

    let mut backend = WaylandBackend::new();
    let result = backend.subscribe();
    assert!(
        result.is_err(),
        "expected subscribe error without Wayland compositor"
    );
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
fn wayland_probe_without_compositor_is_not_automatic() {
    if common::has_wayland_display() {
        return;
    }

    assert!(!matches!(
        WaylandBackend::probe_support(),
        WaylandSupport::Automatic { .. }
    ));
}

#[test]
#[ignore = "requires live Wayland compositor with clipboard protocol implementation"]
fn wayland_read_write_round_trip() {
    if !common::has_wayland_display() {
        return;
    }
    let mut backend = WaylandBackend::new();
    let text = "ccvv-wayland-integration-test-payload";
    match backend.write_plain_text(text) {
        Ok(token) => {
            assert!(token.backend_serial.is_some());
        }
        Err(ccvv_linux::backend::BackendError::Unavailable)
        | Err(ccvv_linux::backend::BackendError::Protocol(_)) => {}
        Err(e) => panic!("unexpected error: {e}"),
    }
}

#[test]
fn wayland_event_stream_observes_clipboard_changes_in_harness() {
    if std::env::var_os("CCVV_WAYLAND_HARNESS").is_none() || !common::has_wayland_display() {
        return;
    }

    let _guard = harness_lock();
    let mut backend = WaylandBackend::new();
    let stream = backend
        .subscribe()
        .expect("wayland harness should expose an event-capable clipboard backend");

    let unique = format!(
        "ccvv-wayland-event-stream-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    backend
        .write_plain_text(&unique)
        .expect("wayland harness should accept clipboard writes");

    let snapshot = match stream.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(snapshot)) => snapshot,
        Ok(Err(error)) => panic!("unexpected backend stream error: {error}"),
        Err(RecvTimeoutError::Timeout) => {
            panic!("timed out waiting for a wayland clipboard event")
        }
        Err(RecvTimeoutError::Disconnected) => {
            panic!("wayland clipboard event stream disconnected unexpectedly")
        }
    };

    assert_eq!(snapshot.acquired_plain_text, unique);
}

#[test]
fn wayland_harness_probe_reports_automatic_support() {
    if std::env::var_os("CCVV_WAYLAND_HARNESS").is_none() || !common::has_wayland_display() {
        return;
    }

    let _guard = harness_lock();
    assert!(matches!(
        WaylandBackend::probe_support(),
        WaylandSupport::Automatic { .. }
    ));
}

#[test]
fn wayland_harness_backend_capability_is_automatic() {
    if std::env::var_os("CCVV_WAYLAND_HARNESS").is_none() || !common::has_wayland_display() {
        return;
    }

    let _guard = harness_lock();
    let backend = WaylandBackend::new();

    assert_eq!(
        backend.capability(),
        ccvv_linux::ui_protocol::BackendCapability::Automatic
    );
}

#[test]
fn wayland_harness_write_returns_backend_serial() {
    if std::env::var_os("CCVV_WAYLAND_HARNESS").is_none() || !common::has_wayland_display() {
        return;
    }

    let _guard = harness_lock();
    let mut backend = WaylandBackend::new();
    let unique = format!(
        "ccvv-wayland-token-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );

    let token = backend
        .write_plain_text(&unique)
        .expect("wayland harness should accept clipboard writes");

    assert!(
        token.backend_serial.is_some(),
        "automatic wayland writes should return a backend serial"
    );
}

#[test]
fn wayland_harness_snapshot_matches_last_written_text() {
    if std::env::var_os("CCVV_WAYLAND_HARNESS").is_none() || !common::has_wayland_display() {
        return;
    }

    let _guard = harness_lock();
    let mut backend = WaylandBackend::new();
    let unique = format!(
        "ccvv-wayland-snapshot-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    backend
        .write_plain_text(&unique)
        .expect("wayland harness should accept clipboard writes");

    let snapshot = backend
        .read_snapshot()
        .expect("wayland harness should expose clipboard snapshots");

    assert_eq!(snapshot.acquired_plain_text, unique);
}
