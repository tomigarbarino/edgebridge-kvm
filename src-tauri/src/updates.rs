use crate::service::Shared;
use std::{sync::Arc, time::Duration};
use tauri::{AppHandle, State};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::sync::Mutex;

struct Download {
    update: Update,
    bytes: Vec<u8>,
}

#[derive(Default)]
pub struct Updates(Mutex<Option<Download>>);

#[tauri::command]
pub async fn check_update(
    app: AppHandle,
    updates: State<'_, Updates>,
) -> Result<Option<String>, String> {
    let mut staged = updates.0.lock().await;
    if let Some(download) = staged.as_ref() {
        return Ok(Some(download.update.version.clone()));
    }

    let updater = app
        .updater_builder()
        .timeout(Duration::from_secs(45))
        .build()
        .map_err(|err| err.to_string())?;
    let Some(update) = updater.check().await.map_err(|err| err.to_string())? else {
        return Ok(None);
    };
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|err| err.to_string())?;
    let version = update.version.clone();
    *staged = Some(Download { update, bytes });
    Ok(Some(version))
}

#[tauri::command]
pub async fn install_update(
    version: String,
    updates: State<'_, Updates>,
    shared: State<'_, Arc<Shared>>,
) -> Result<(), String> {
    let staged = updates.0.lock().await;
    let download = staged
        .as_ref()
        .filter(|item| item.update.version == version)
        .ok_or("Check for the update again.")?;
    let _install = shared.reserve_install()?;
    download
        .update
        .install(&download.bytes)
        .map_err(|err| err.to_string())
}
