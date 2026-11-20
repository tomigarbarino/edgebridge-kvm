#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
mod clipboard;
#[cfg(windows)]
mod diagnostics;
#[cfg(windows)]
mod discovery;
mod layout;
#[cfg(windows)]
mod native;
#[cfg(windows)]
mod network;
mod pairing;
mod protocol;
mod receiver;
#[cfg(windows)]
mod service;
#[cfg(windows)]
mod session_health;
mod state;
#[cfg(windows)]
mod updates;

#[cfg(windows)]
fn main() {
    use std::sync::Arc;
    use tauri::Manager;

    native::configure_dpi();
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updates::Updates::default())
        .manage(Arc::new(service::Shared::default()))
        .invoke_handler(tauri::generate_handler![
            service::status,
            service::stop,
            service::listen,
            service::connect,
            service::discover,
            diagnostics::diagnose_connection,
            diagnostics::copy_connection_diagnostic,
            service::request_pairing,
            service::decide_pairing,
            updates::check_update,
            updates::install_update,
            native::restore_cursor
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let window = window.clone();
                let shared = Arc::clone(window.state::<Arc<service::Shared>>().inner());
                tauri::async_runtime::spawn(async move {
                    shared.stop().await;
                    let _ = window.destroy();
                });
            }
        })
        .run(tauri::generate_context!())
        .expect("EdgeBridge could not start");
}

#[cfg(not(windows))]
fn main() {
    eprintln!("EdgeBridge KVM currently requires Windows.");
}
