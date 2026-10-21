// The C library's soname: Qt links `libfernicons.so` from target/release
// in development and from /usr/lib on the image, and finds it by this name.
fn main() {
    println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,libfernicons.so");
    println!("cargo:rerun-if-changed=build.rs");
}
