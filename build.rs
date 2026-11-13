use std::path::Path;

fn main() {
    // rust-embed requires the assets folder to exist at compile time; a fresh
    // clone without a frontend build gets an empty folder (SPA not mounted).
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("web/dist");
    let _ = std::fs::create_dir_all(&dist);
    println!("cargo:rerun-if-changed=web/dist");
}
