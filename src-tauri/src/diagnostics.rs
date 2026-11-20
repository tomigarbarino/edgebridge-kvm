use std::{
    io,
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use serde::Serialize;
use tokio::{net::TcpStream, time::timeout};

use crate::{
    discovery,
    protocol::TCP_PORT,
    service::{Shared, Status},
};

#[derive(Serialize)]
pub struct Report {
    version: &'static str,
    role: &'static str,
    phase: &'static str,
    confirmation: &'static str,
    receiver_discovery: Option<bool>,
    discovery: Option<Discovery>,
    tcp: TcpResult,
}

#[derive(Serialize)]
struct Discovery {
    available: bool,
    interfaces: Vec<discovery::InterfaceResult>,
    receiver_count: usize,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum TcpResult {
    NotNeeded,
    NeedAddress,
    ChooseReceiver,
    InvalidAddress,
    Reachable,
    Refused,
    TimedOut,
    Unreachable,
}

#[tauri::command]
pub async fn diagnose_connection(
    shared: tauri::State<'_, Arc<Shared>>,
    primary: bool,
    address: String,
) -> Result<Report, String> {
    Ok(build_report(shared.snapshot(), primary, &address).await)
}

async fn build_report(status: Status, primary: bool, address: &str) -> Report {
    let role = status
        .role
        .unwrap_or(if primary { "primary" } else { "secondary" });
    let mut report = Report {
        version: env!("CARGO_PKG_VERSION"),
        role,
        phase: status.phase,
        confirmation: match status.approval {
            Some(approval) if approval.accepted => "waiting_for_peer",
            Some(_) => "required",
            None => "none",
        },
        receiver_discovery: (role == "secondary" && status.phase == "waiting")
            .then_some(status.discovery_listening),
        discovery: None,
        tcp: TcpResult::NotNeeded,
    };
    if role == "secondary" || status.phase == "connected" || report.confirmation != "none" {
        return report;
    }

    let (receivers, interfaces, available) = match discovery::scan_report().await {
        Ok(scan) => (scan.receivers, scan.interfaces, true),
        Err(_) => (vec![], vec![], false),
    };
    let target = if address.trim().is_empty() {
        match receivers.as_slice() {
            [] => Err(TcpResult::NeedAddress),
            [receiver] => Ok(receiver.address),
            _ => Err(TcpResult::ChooseReceiver),
        }
    } else {
        address
            .trim()
            .parse()
            .map_err(|_| TcpResult::InvalidAddress)
    };
    report.discovery = Some(Discovery {
        available,
        interfaces,
        receiver_count: receivers.len(),
    });
    report.tcp = match target {
        Ok(ip) if usable_address(ip) => probe_tcp((ip, TCP_PORT).into()).await,
        Ok(_) => TcpResult::InvalidAddress,
        Err(result) => result,
    };
    report
}

fn usable_address(ip: Ipv4Addr) -> bool {
    !(ip.is_unspecified() || ip.is_loopback() || ip.is_broadcast() || ip.is_multicast())
}

async fn probe_tcp(address: SocketAddr) -> TcpResult {
    match timeout(Duration::from_secs(2), TcpStream::connect(address)).await {
        Ok(Ok(_)) => TcpResult::Reachable,
        Ok(Err(err)) if err.kind() == io::ErrorKind::ConnectionRefused => TcpResult::Refused,
        Ok(Err(err)) if err.kind() != io::ErrorKind::TimedOut => TcpResult::Unreachable,
        _ => TcpResult::TimedOut,
    }
}

#[tauri::command]
pub fn copy_connection_diagnostic(report: String) -> Result<(), String> {
    if report.len() > 8192 || !report.starts_with("EdgeBridge connection diagnostic\n") {
        return Err("Invalid diagnostic text.".into());
    }
    clipboard_win::set_clipboard(clipboard_win::formats::Unicode, report)
        .map_err(|_| "Windows clipboard is busy. Try again.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn distinguishes_an_open_tcp_port_from_a_closed_one() -> anyhow::Result<()> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        assert_eq!(probe_tcp(address).await, TcpResult::Reachable);
        drop(listener);
        // A local firewall may drop the closed-port probe instead of replying with a reset.
        assert_ne!(probe_tcp(address).await, TcpResult::Reachable);
        Ok(())
    }

    #[tokio::test]
    async fn report_excludes_pairing_secrets_and_does_not_probe_while_awaiting_confirmation() {
        let status = Status {
            phase: "starting",
            role: Some("primary"),
            code: Some("manual-secret".into()),
            peer: Some("192.168.1.42".into()),
            message: Some("raw Windows and network error".into()),
            approval: Some(crate::service::Approval {
                id: 5,
                name: "Private-PC".into(),
                comparison: "12345678".into(),
                accepted: false,
            }),
            ..Status::default()
        };
        let report = build_report(status, true, "192.168.1.42").await;
        assert!(report.discovery.is_none());
        assert_eq!(report.tcp, TcpResult::NotNeeded);
        assert_eq!(report.confirmation, "required");
        let text = serde_json::to_string(&report).unwrap();
        for excluded in [
            "manual-secret",
            "192.168.1.42",
            "raw Windows",
            "Private-PC",
            "12345678",
        ] {
            assert!(!text.contains(excluded));
        }
    }
}
