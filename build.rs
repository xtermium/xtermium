fn main() {
    // The release workflow's build number (About, update checks): absent in local builds.
    println!("cargo:rerun-if-env-changed=XTERMIUM_BUILD");
    tauri_build::build()
}
