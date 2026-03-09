mod common;

#[test]
fn wayland_integration_smoke_is_gated_by_environment() {
    if common::has_wayland_display() {
        println!("wayland integration session available");
    } else {
        println!("{}", common::skip_message("wayland"));
    }
}

#[test]
#[ignore = "requires Wayland compositor with data-control protocol"]
fn wayland_integration_smoke() {
    // TODO: add ext-data-control-v1 and wlr-data-control-unstable-v1 coverage.
}
