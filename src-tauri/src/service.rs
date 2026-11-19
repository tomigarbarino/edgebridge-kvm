use std::{
    net::Ipv4Addr,
    sync::{Arc, Mutex},
};

use crate::{discovery, network, pairing::Invite, protocol::Edge};
use serde::Serialize;

#[derive(Clone, Serialize)]
pub struct Status {
    pub phase: &'static str,
    pub role: Option<&'static str>,
    pub code: Option<String>,
    pub address: Option<String>,
    pub peer: Option<String>,
    pub latency_ms: Option<u32>,
    pub forwarding: bool,
    pub message: Option<String>,
}

impl Default for Status {
    fn default() -> Self {
        Self {
            phase: "idle",
            role: None,
            code: None,
            address: None,
            peer: None,
            latency_ms: None,
            forwarding: false,
            message: None,
        }
    }
}

#[derive(Default)]
struct State {
    generation: u64,
    installing: bool,
    task: Option<tauri::async_runtime::JoinHandle<()>>,
    status: Status,
}

#[derive(Default)]
pub struct Shared(Mutex<State>, tokio::sync::Mutex<()>);

impl Shared {
    pub fn update(&self, generation: u64, change: impl FnOnce(&mut Status)) {
        let mut state = self.0.lock().unwrap();
        if state.generation == generation {
            change(&mut state.status);
        }
    }

    fn launch<F>(
        self: &Arc<Self>,
        role: &'static str,
        execute: impl FnOnce(Arc<Self>, u64) -> F + Send + 'static,
    ) -> Result<(), String>
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let mut state = self.0.lock().unwrap();
        if state.installing {
            return Err("An update is being installed.".into());
        }
        if state.status.role.is_some() || state.status.phase == "stopping" {
            return Err("Stop the current session first.".into());
        }
        state.generation = state.generation.wrapping_add(1);
        let generation = state.generation;
        state.status = Status {
            phase: "starting",
            role: Some(role),
            ..Status::default()
        };
        let shared = Arc::clone(self);
        state.task = Some(tauri::async_runtime::spawn(async move {
            if let Err(err) = execute(Arc::clone(&shared), generation).await {
                shared.update(generation, |status| {
                    *status = Status {
                        phase: "error",
                        message: Some(err.to_string()),
                        ..Status::default()
                    };
                });
            }
        }));
        Ok(())
    }

    pub fn reserve_install(&self) -> Result<InstallGuard<'_>, String> {
        let mut state = self.0.lock().unwrap();
        if state.status.role.is_some() || state.status.phase == "stopping" {
            return Err("Stop the current session before installing an update.".into());
        }
        if state.installing {
            return Err("An update is already being installed.".into());
        }
        state.installing = true;
        Ok(InstallGuard { shared: self })
    }

    pub async fn stop(&self) {
        let _stop = self.1.lock().await;
        let (generation, task) = {
            let mut state = self.0.lock().unwrap();
            state.generation = state.generation.wrapping_add(1);
            state.status.phase = "stopping";
            (state.generation, state.task.take())
        };
        if let Some(task) = task {
            task.abort();
            let _ = task.await;
        }
        self.update(generation, |status| *status = Status::default());
    }
}

pub struct InstallGuard<'a> {
    shared: &'a Shared,
}

impl Drop for InstallGuard<'_> {
    fn drop(&mut self) {
        self.shared.0.lock().unwrap().installing = false;
    }
}

#[tauri::command]
pub fn status(shared: tauri::State<'_, Arc<Shared>>) -> Status {
    shared.0.lock().unwrap().status.clone()
}

#[tauri::command]
pub async fn stop(shared: tauri::State<'_, Arc<Shared>>) -> Result<(), String> {
    shared.stop().await;
    Ok(())
}

#[tauri::command]
pub fn listen(shared: tauri::State<'_, Arc<Shared>>) -> Result<(), String> {
    shared.inner().launch("secondary", network::listen)
}

#[tauri::command]
pub fn connect(
    shared: tauri::State<'_, Arc<Shared>>,
    address: String,
    code: String,
    from: Edge,
    to: Edge,
) -> Result<(), String> {
    let ip: Ipv4Addr = address
        .trim()
        .parse()
        .map_err(|_| "Enter the receiver's IPv4 address.")?;
    if ip.is_unspecified() || ip.is_multicast() || ip.is_broadcast() || ip.is_loopback() {
        return Err("Enter the IPv4 address of the other computer.".into());
    }
    let invite = Invite::parse(&code).map_err(|_| "The pairing code is invalid.")?;
    shared.inner().launch("primary", move |shared, generation| {
        network::send(shared, generation, ip, invite, from, to)
    })
}

#[tauri::command]
pub async fn discover() -> Result<Vec<String>, String> {
    discovery::scan().await.map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_guard_excludes_sessions_and_other_installs() {
        let shared = Arc::new(Shared::default());
        {
            let _install = shared.reserve_install().expect("idle app can update");
            assert!(shared.reserve_install().is_err());
            assert!(shared.launch("primary", |_, _| async { Ok(()) }).is_err());
        }
        shared.0.lock().unwrap().status.role = Some("primary");
        assert!(shared.reserve_install().is_err());
        shared.0.lock().unwrap().status = Status::default();
        assert!(shared.reserve_install().is_ok());
    }
}
