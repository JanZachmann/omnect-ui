use std::path::Path;

fn main() {
    // Tell Cargo to only rerun this build script if specific files change
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../ui/dist");

    // Path to the Vue dist folder (relative to backend Cargo.toml)
    let ui_dist_path = Path::new("../ui/dist");

    assert!(
        ui_dist_path.exists(),
        "UI dist folder not found at {}. Please build the frontend first with: cd ../ui && pnpm run build",
        ui_dist_path.display()
    );

    static_files::resource_dir(ui_dist_path)
        .build()
        .expect("Failed to build static resources");
}
