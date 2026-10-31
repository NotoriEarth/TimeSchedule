fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let root = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
        let manifest = root.join("../../src-tauri/app.manifest");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());

        // Keep Explorer's version properties in sync with the version shown in the app.
        let version = std::env::var("CARGO_PKG_VERSION").unwrap();
        let numeric = ["MAJOR", "MINOR", "PATCH"].map(|part| {
            std::env::var(format!("CARGO_PKG_VERSION_{part}"))
                .unwrap()
                .parse::<u16>()
                .expect("Windows version components must fit in 16 bits")
        });
        let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
        let rc = out.join("version.rc");
        let res = out.join("version.res");
        std::fs::write(
            &rc,
            format!(
                r#"1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEFLAGSMASK 0x3fL
FILEFLAGS 0
FILEOS 0x40004L
FILETYPE 0x1L
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "FileDescription", "TimeSchedule"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "timeschedule-native"
      VALUE "OriginalFilename", "TimeSchedule.exe"
      VALUE "ProductName", "TimeSchedule"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x0409, 1200
  END
END
"#,
                major = numeric[0],
                minor = numeric[1],
                patch = numeric[2],
            ),
        )
        .expect("Could not write Windows version resource");
        let status = std::process::Command::new("rc.exe")
            .arg("/nologo")
            .arg("/fo")
            .arg(&res)
            .arg(&rc)
            .status()
            .expect("Windows SDK rc.exe is required; use scripts/build-native.ps1");
        assert!(
            status.success(),
            "Windows version resource compilation failed"
        );
        println!("cargo:rustc-link-arg={}", res.display());
    }
}
