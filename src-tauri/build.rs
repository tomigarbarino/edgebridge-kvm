fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "status",
            "stop",
            "listen",
            "connect",
            "discover",
            "check_update",
            "install_update",
        ]),
    ))
    .expect("EdgeBridge build configuration is invalid");
}
