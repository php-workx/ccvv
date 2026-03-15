fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=WAYLAND_DISPLAY");
    println!("cargo:rerun-if-env-changed=XDG_CURRENT_DESKTOP");
}
