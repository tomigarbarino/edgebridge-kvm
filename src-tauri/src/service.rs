use std::{
    net::Ipv4Addr,
    sync::{Arc, Mutex},
};

use crate::{discovery, network, pairing::Invite, protocol::Edge, trust};
use serde::Serialize;
use tokio::sync::{oneshot, watch};

#[derive(Clone, Serialize)]
pub struct Approval {
    pub id: u32,
    pub name: String,
    pub comparison: String,
    pub accepted: bool,
    pub can_remember: bool,
}

pub struct Decision {
    pub accepted: bool,
    pub remember: bool,
}

#[derive(Clone, Serialize)]
pub struct Status {
    pub phase: &'static str,
    pub role: Option<&'static str>,
    pub code: Option<String>,
    pub address: Option<String>,
    pub peer: Option<String>,
    pub latency_ms: Option<u32>,
    pub forwarding: bool,
    pub clipboard_active: bool,
    pub clipboard_live: bool,
    pub discovery_listening: bool,
    pub approval: Option<Approval>,
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
            clipboard_active: false,
            clipboard_live: false,
            discovery_listening: false,
            approval: None,
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
    next_approval: u32,
    decision: Option<oneshot::Sender<Decision>>,
}

#[derive(Default)]
pub struct Shared(
    Mutex<State>,
    tokio::sync::Mutex<()>,
    watch::Sender<bool>,
    pub trust::Store,
);

impl Shared {
    pub fn with_trust(path: std::path::PathBuf) -> Self {
        Self(
            Mutex::default(),
            tokio::sync::Mutex::default(),
            watch::Sender::default(),
            trust::Store::new(path),
        )
    }

    pub fn clipboard_preference(&self) -> watch::Receiver<bool> {
        self.2.subscribe()
    }

    pub fn snapshot(&self) -> Status {
        self.0.lock().unwrap().status.clone()
    }

    pub fn ready_to_pair(&self, generation: u64) -> bool {
        let state = self.0.lock().unwrap();
        state.generation == generation
            && state.status.phase == "waiting"
            && state.status.approval.is_none()
    }

    pub fn decide(&self, id: u32, accept: bool, remember: bool) -> Result<(), String> {
        let mut state = self.0.lock().unwrap();
        let approval = state
            .status
            .approval
            .as_ref()
            .filter(|approval| approval.id == id)
            .ok_or("This pairing request has expired.")?;
        let remember = accept && remember && approval.can_remember;
        let sender = state
            .decision
            .take()
            .ok_or("This request was already answered.")?;
        sender
            .send(Decision {
                accepted: accept,
                remember,
            })
            .map_err(|_| "This pairing request has expired.")?;
        if let Some(approval) = &mut state.status.approval {
            approval.accepted = accept;
        }
        Ok(())
    }

    pub fn prompt(
        &self,
        generation: u64,
        name: String,
        comparison: String,
        can_remember: bool,
    ) -> anyhow::Result<(ApprovalGuard<'_>, oneshot::Receiver<Decision>)> {
        let mut state = self.0.lock().unwrap();
        anyhow::ensure!(
            state.generation == generation && state.status.approval.is_none(),
            "Another pairing request is already pending"
        );
        state.next_approval = state.next_approval.wrapping_add(1);
        let id = state.next_approval;
        let (send, receive) = oneshot::channel();
        state.decision = Some(send);
        state.status.approval = Some(Approval {
            id,
            name,
            comparison,
            accepted: false,
            can_remember,
        });
        Ok((
            ApprovalGuard {
                shared: self,
                generation,
                id,
            },
            receive,
        ))
    }

    pub fn update(&self, generation: u64, change: impl FnOnce(&mut Status)) {
        let mut state = self.0.lock().unwrap();
        if state.generation == generation {
            change(&mut state.status);
        }
    }

    fn launch<F>(
        self: &Arc<Self>,
        role: &'static str,
        share_clipboard: bool,
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
        self.2.send_replace(share_clipboard);
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
            state.status.approval = None;
            state.decision = None;
            (state.generation, state.task.take())
        };
        if let Some(task) = task {
            task.abort();
            let _ = task.await;
        }
        self.update(generation, |status| *status = Status::default());
    }
}

pub struct ApprovalGuard<'a> {
    shared: &'a Shared,
    generation: u64,
    id: u32,
}

impl Drop for ApprovalGuard<'_> {
    fn drop(&mut self) {
        let mut state = self.shared.0.lock().unwrap();
        if state.generation == self.generation
            && state
                .status
                .approval
                .as_ref()
                .is_some_and(|approval| approval.id == self.id)
        {
            state.status.approval = None;
            state.decision = None;
        }
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
    shared.snapshot()
}

#[tauri::command]
pub fn set_clipboard_sharing(
    shared: tauri::State<'_, Arc<Shared>>,
    enabled: bool,
) -> Result<(), String> {
    let state = shared.0.lock().unwrap();
    if state.status.phase != "connected" || !state.status.clipboard_live {
        return Err("Live clipboard sharing requires an active connection on both PCs.".into());
    }
    shared.2.send_replace(enabled);
    Ok(())
}

#[tauri::command]
pub async fn stop(shared: tauri::State<'_, Arc<Shared>>) -> Result<(), String> {
    shared.stop().await;
    Ok(())
}

#[tauri::command]
pub fn listen(shared: tauri::State<'_, Arc<Shared>>, share_clipboard: bool) -> Result<(), String> {
    shared
        .inner()
        .launch("secondary", share_clipboard, move |shared, generation| {
            network::listen(shared, generation, share_clipboard)
        })
}

#[tauri::command]
pub fn connect(
    shared: tauri::State<'_, Arc<Shared>>,
    address: String,
    code: String,
    from: Edge,
    to: Edge,
    share_clipboard: bool,
) -> Result<(), String> {
    let ip: Ipv4Addr = address
        .trim()
        .parse()
        .map_err(|_| "Enter the receiver's IPv4 address.")?;
    if ip.is_unspecified() || ip.is_multicast() || ip.is_broadcast() || ip.is_loopback() {
        return Err("Enter the IPv4 address of the other computer.".into());
    }
    let invite = Invite::parse(&code).map_err(|_| "The pairing code is invalid.")?;
    shared
        .inner()
        .launch("primary", share_clipboard, move |shared, generation| {
            network::send(shared, generation, ip, invite, (from, to), share_clipboard)
        })
}

#[tauri::command]
pub async fn discover() -> Result<Vec<discovery::Receiver>, String> {
    discovery::scan().await.map_err(|err| err.to_string())
}

#[tauri::command]
pub fn decide_pairing(
    shared: tauri::State<'_, Arc<Shared>>,
    id: u32,
    accept: bool,
    remember: bool,
) -> Result<(), String> {
    shared.decide(id, accept, remember)
}

#[tauri::command]
pub fn trusted_device(shared: tauri::State<'_, Arc<Shared>>) -> Result<Option<String>, String> {
    shared
        .3
        .load()
        .map(|peer| peer.map(|peer| peer.name))
        .map_err(|_| "Could not read the remembered PC. You can forget it and pair again.".into())
}

#[tauri::command]
pub fn forget_device(shared: tauri::State<'_, Arc<Shared>>) -> Result<(), String> {
    let state = shared.0.lock().unwrap();
    if state.status.role.is_some() || state.status.phase == "stopping" {
        return Err("Stop the current session before forgetting the PC.".into());
    }
    shared
        .3
        .forget()
        .map_err(|_| "Could not forget the remembered PC.".into())
}

#[tauri::command]
pub fn request_pairing(
    shared: tauri::State<'_, Arc<Shared>>,
    address: Ipv4Addr,
    advertisement: discovery::Advertisement,
    from: Edge,
    to: Edge,
    share_clipboard: bool,
) -> Result<(), String> {
    if address.is_unspecified()
        || address.is_multicast()
        || address.is_broadcast()
        || address.is_loopback()
        || !discovery::valid_name(&advertisement.name)
    {
        return Err("Choose a valid PC on the network.".into());
    }
    shared
        .inner()
        .launch("primary", share_clipboard, move |shared, generation| {
            network::request_pairing(
                shared,
                generation,
                address,
                advertisement,
                (from, to),
                share_clipboard,
            )
        })
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
            assert!(
                shared
                    .launch("primary", false, |_, _| async { Ok(()) })
                    .is_err()
            );
        }
        shared.0.lock().unwrap().status.role = Some("primary");
        assert!(shared.reserve_install().is_err());
        shared.0.lock().unwrap().status = Status::default();
        assert!(shared.reserve_install().is_ok());
    }
}
