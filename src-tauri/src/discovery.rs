use std::{
    collections::{BTreeMap, BTreeSet},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    time::Duration,
};

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use tokio::{
    net::UdpSocket,
    task::JoinSet,
    time::{Instant, timeout_at},
};

use crate::protocol::DISCOVERY_PORT;

const QUERY: &[u8] = b"EdgeBridge/1 find";
const RESPONSE: &[u8] = b"EdgeBridge/1 receiver";
const NAMED_QUERY: &[u8] = b"EdgeBridge/3 find";
const NAMED_RESPONSE: &[u8] = b"EdgeBridge/3 receiver ";

#[derive(Clone, Serialize, Deserialize)]
pub struct Advertisement {
    pub name: String,
    pub pin: [u8; 32],
}

#[derive(Serialize)]
pub struct Receiver {
    pub address: Ipv4Addr,
    pub advertisement: Option<Advertisement>,
}

pub fn computer_name() -> String {
    std::env::var("COMPUTERNAME")
        .ok()
        .filter(|name| valid_name(name))
        .unwrap_or_else(|| "Windows PC".into())
}

pub fn valid_name(name: &str) -> bool {
    !name.trim().is_empty()
        && name.len() <= 48
        && name
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b' ' | b'-' | b'_'))
}

pub async fn socket() -> Result<UdpSocket> {
    Ok(UdpSocket::bind((Ipv4Addr::UNSPECIFIED, DISCOVERY_PORT)).await?)
}

pub async fn serve(
    socket: UdpSocket,
    advertisement: Advertisement,
    available: impl Fn() -> bool,
) -> Result<()> {
    let mut response = NAMED_RESPONSE.to_vec();
    response.extend(serde_json::to_vec(&advertisement)?);
    let mut incoming = [0; 64];
    loop {
        let (size, address) = socket.recv_from(&mut incoming).await?;
        if &incoming[..size] == QUERY {
            socket.send_to(RESPONSE, address).await?;
        } else if &incoming[..size] == NAMED_QUERY && available() {
            socket.send_to(&response, address).await?;
        }
    }
}

pub async fn scan() -> Result<Vec<Receiver>> {
    let local = crate::native::lan_addresses()?;
    let targets = local.iter().filter_map(|&(source, prefix)| {
        broadcast_for(source, prefix)
            .map(|broadcast| (source, SocketAddrV4::new(broadcast, DISCOVERY_PORT)))
    });
    let local_ips = local.iter().map(|&(ip, _)| ip).collect::<BTreeSet<_>>();
    scan_targets(targets, &local_ips).await
}

async fn scan_targets(
    targets: impl IntoIterator<Item = (Ipv4Addr, SocketAddrV4)>,
    local_ips: &BTreeSet<Ipv4Addr>,
) -> Result<Vec<Receiver>> {
    let mut scans = JoinSet::new();
    let deadline = Instant::now() + Duration::from_millis(1200);
    for (source, target) in targets {
        scans.spawn(scan_interface(source, target, deadline));
    }
    let mut found = BTreeMap::new();
    let mut working_interfaces = 0;
    while let Some(result) = scans.join_next().await {
        let Ok(Ok(receivers)) = result else { continue };
        working_interfaces += 1;
        for (address, advertisement) in receivers {
            if !local_ips.contains(&address) {
                insert_receiver(&mut found, address, advertisement);
            }
        }
    }
    ensure!(
        working_interfaces > 0,
        "No LAN interface could send a discovery request."
    );
    Ok(found
        .into_iter()
        .map(|(address, advertisement)| Receiver {
            address,
            advertisement,
        })
        .collect())
}

fn broadcast_for(source: Ipv4Addr, prefix: u8) -> Option<Ipv4Addr> {
    if !(1..=30).contains(&prefix)
        || source.is_loopback()
        || source.is_unspecified()
        || source.is_multicast()
    {
        return None;
    }
    let host_mask = u32::MAX >> prefix;
    let bits = u32::from(source);
    let host = bits & host_mask;
    (host != 0 && host != host_mask).then(|| Ipv4Addr::from(bits | host_mask))
}

fn insert_receiver(
    found: &mut BTreeMap<Ipv4Addr, Option<Advertisement>>,
    address: Ipv4Addr,
    advertisement: Option<Advertisement>,
) {
    if found.len() < 32 || found.contains_key(&address) {
        let existing = found.entry(address).or_insert(None);
        if advertisement.is_some() {
            *existing = advertisement;
        }
    }
}

async fn scan_interface(
    source: Ipv4Addr,
    target: SocketAddrV4,
    deadline: Instant,
) -> Result<BTreeMap<Ipv4Addr, Option<Advertisement>>> {
    let socket = UdpSocket::bind((source, 0)).await?;
    socket.set_broadcast(true)?;
    socket.send_to(QUERY, target).await?;
    socket.send_to(NAMED_QUERY, target).await?;
    let mut incoming = [0; 512];
    let mut found = BTreeMap::new();
    while let Ok(reply) = timeout_at(deadline, socket.recv_from(&mut incoming)).await {
        let (size, address) = reply?;
        if let SocketAddr::V4(address) = address
            && address.port() == target.port()
            && *address.ip() != source
        {
            let response = &incoming[..size];
            if response == RESPONSE {
                insert_receiver(&mut found, *address.ip(), None);
            } else if let Some(json) = response.strip_prefix(NAMED_RESPONSE)
                && let Ok(advertisement) = serde_json::from_slice::<Advertisement>(json)
                && valid_name(&advertisement.name)
            {
                insert_receiver(&mut found, *address.ip(), Some(advertisement));
            }
        }
    }
    Ok(found)
}

pub fn suggested_address() -> Option<String> {
    let socket = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    // UDP connect selects a local route without sending any packets.
    socket
        .connect((Ipv4Addr::new(192, 0, 2, 1), DISCOVERY_PORT))
        .ok()?;
    Some(socket.local_addr().ok()?.ip().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::timeout;

    #[test]
    fn uses_each_interfaces_subnet_broadcast() {
        assert_eq!(
            broadcast_for(Ipv4Addr::new(192, 168, 0, 8), 24),
            Some(Ipv4Addr::new(192, 168, 0, 255))
        );
        assert_eq!(
            broadcast_for(Ipv4Addr::new(172, 20, 16, 1), 20),
            Some(Ipv4Addr::new(172, 20, 31, 255))
        );
        assert_eq!(broadcast_for(Ipv4Addr::new(10, 0, 0, 1), 31), None);
        assert_eq!(broadcast_for(Ipv4Addr::LOCALHOST, 8), None);
    }

    #[tokio::test]
    async fn keeps_replies_on_the_working_interface_when_another_bind_fails() -> Result<()> {
        let peer = Ipv4Addr::new(127, 0, 0, 2);
        let listener = UdpSocket::bind((peer, 0)).await?;
        let target = SocketAddrV4::new(peer, listener.local_addr()?.port());
        let server = tokio::spawn(serve(
            listener,
            Advertisement {
                name: "Other-PC".into(),
                pin: [19; 32],
            },
            || true,
        ));
        let result = scan_targets(
            [
                (Ipv4Addr::new(192, 0, 2, 1), target),
                (Ipv4Addr::LOCALHOST, target),
            ],
            &[Ipv4Addr::LOCALHOST].into_iter().collect(),
        )
        .await;
        server.abort();
        let receivers = result?;
        assert_eq!(receivers.len(), 1);
        assert_eq!(receivers[0].address, peer);
        assert_eq!(
            receivers[0].advertisement.as_ref().unwrap().name,
            "Other-PC"
        );
        Ok(())
    }

    #[tokio::test]
    async fn advertises_only_to_the_query_source() -> Result<()> {
        let listener = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let sender = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let server = tokio::spawn(serve(
            listener,
            Advertisement {
                name: "Test-PC".into(),
                pin: [42; 32],
            },
            || true,
        ));
        let mut buf = [0; 64];
        let result = async {
            sender.send_to(b"unrecognized", address).await?;
            assert!(
                timeout(Duration::from_millis(30), sender.recv_from(&mut buf))
                    .await
                    .is_err()
            );
            sender.send_to(QUERY, address).await?;
            let (size, source) =
                timeout(Duration::from_secs(2), sender.recv_from(&mut buf)).await??;
            assert_eq!(source, address);
            assert_eq!(&buf[..size], RESPONSE);
            sender.send_to(NAMED_QUERY, address).await?;
            let mut named = [0; 512];
            let (size, source) =
                timeout(Duration::from_secs(2), sender.recv_from(&mut named)).await??;
            assert_eq!(source, address);
            let data = named[..size].strip_prefix(NAMED_RESPONSE).unwrap();
            let advertisement: Advertisement = serde_json::from_slice(data)?;
            assert_eq!(advertisement.name, "Test-PC");
            assert_eq!(advertisement.pin, [42; 32]);
            Ok(())
        }
        .await;
        server.abort();
        result
    }
}
