//! Rebuild the generated bindings when the protocol XML changes (the
//! scanner's macros read it, which Cargo cannot see by itself).

fn main() {
    println!("cargo:rerun-if-changed=fern-wm-v1.xml");
}
