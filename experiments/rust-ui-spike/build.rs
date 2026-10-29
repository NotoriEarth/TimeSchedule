fn main() {
    // Spike-only bridge: exercise the existing guarded Win32 implementation
    // without linking Tauri or changing the production app. Fail if its API moves.
    let root = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let source = root.join("../../src-tauri/src/desktop.rs");
    println!("cargo:rerun-if-changed={}", source.display());
    let mut code = std::fs::read_to_string(source).unwrap();
    for (from, to) in [
        (
            "use tauri::WebviewWindow;",
            "use crate::windows_probe::NativeWindow as WebviewWindow;",
        ),
        (
            "#[derive(Clone, Debug, serde::Serialize)]",
            "#[derive(Clone, Debug)]",
        ),
        ("#[serde(rename_all = \"camelCase\")]", ""),
        // Pinned winit 0.30.13 registers this class without CS_OWNDC/CS_CLASSDC.
        // Never register/change its class; the original per-HWND safety checks remain.
        (
            "set_native_opacity(handle, percent, WIDGET_CLASS)",
            "set_native_opacity(handle, percent, \"Window Class\")",
        ),
    ] {
        assert_eq!(
            code.matches(from).count(),
            1,
            "desktop bridge needs review: {from}"
        );
        code = code.replace(from, to);
    }
    code = code
        .lines()
        .map(|line| {
            if let Some(doc) = line.strip_prefix("//!") {
                format!("//{doc}\n")
            } else {
                format!("{line}\n")
            }
        })
        .collect();
    std::fs::write(
        std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("desktop_bridge.rs"),
        code,
    )
    .unwrap();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("../../src-tauri/app.manifest");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
}
