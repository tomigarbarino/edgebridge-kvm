#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
mod discovery;
#[cfg(windows)]
mod native;
#[cfg(windows)]
mod network;
mod pairing;
mod protocol;
mod receiver;
#[cfg(windows)]
mod service;
mod state;

#[cfg(windows)]
fn main() {
    use std::sync::Arc;
    use tauri::Manager;

    native::configure_dpi();
    tauri::Builder::default()
        .manage(Arc::new(service::Shared::default()))
        .invoke_handler(tauri::generate_handler![
            service::status,
            service::stop,
            service::listen,
            service::connect,
            service::discover
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
