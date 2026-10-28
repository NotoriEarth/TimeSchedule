fn main() {
    println!("cargo:rerun-if-changed=app.manifest");
    let windows = tauri_build::WindowsAttributes::new().app_manifest(include_str!("app.manifest"));
    let attributes = tauri_build::Attributes::new().windows_attributes(windows);
    tauri_build::try_build(attributes).expect("failed to prepare TimeSchedule build resources");

    if std::env::var("TARGET")
        .map(|target| target.contains("windows-msvc"))
        .unwrap_or(false)
    {
        // Tauri embeds resources into application binaries only. The separate
        // native integration test needs the same Windows 8+ declaration. Keep
        // these flags off application binaries, which already embed a manifest.
        let manifest = std::path::PathBuf::from(
            std::env::var_os("CARGO_MANIFEST_DIR").expect("missing package directory"),
        )
        .join("app.manifest");
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-tests=/MANIFESTINPUT:{}",
            manifest.display()
        );
    }
}
