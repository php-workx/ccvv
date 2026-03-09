mod common;

#[test]
fn x11_integration_smoke_is_gated_by_environment() {
    if common::has_x11_display() {
        println!("x11 integration session available");
    } else {
        println!("{}", common::skip_message("x11"));
    }
}

#[test]
#[ignore = "requires X11 compositor and X11 clipboard integration test setup"]
fn x11_integration_smoke() {
    // TODO: add end-to-end X11 clipboard observation and INCR path checks.
}

#[test]
#[ignore = "requires X11 compositor and integration fixtures"]
fn x11_integration_incr_round_trip() {
    // TODO: add INCR receive/send coverage once X11 backend backend support is implemented.
}
