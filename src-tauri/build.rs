fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "status",
            "set_clipboard_sharing",
            "trusted_device",
            "forget_device",
            "stop",
            "listen",
            "connect",
            "discover",
            "diagnose_connection",
            "copy_connection_diagnostic",
            "request_pairing",
            "decide_pairing",
            "check_update",
            "install_update",
            "restore_cursor",
        ]),
    ))
    .expect("EdgeBridge build configuration is invalid");
}
