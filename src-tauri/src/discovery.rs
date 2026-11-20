use std::{
    collections::BTreeMap,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    time::Duration,
};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::{
    net::UdpSocket,
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
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
    socket.set_broadcast(true)?;
    let target = SocketAddrV4::new(Ipv4Addr::BROADCAST, DISCOVERY_PORT);
    socket.send_to(QUERY, target).await?;
    socket.send_to(NAMED_QUERY, target).await?;
    let deadline = Instant::now() + Duration::from_millis(850);
    let mut incoming = [0; 512];
    let mut found = BTreeMap::new();
    while let Ok(reply) = timeout_at(deadline, socket.recv_from(&mut incoming)).await {
        let (size, address) = reply?;
        if let SocketAddr::V4(address) = address
            && address.port() == DISCOVERY_PORT
            && (found.len() < 32 || found.contains_key(address.ip()))
        {
            let response = &incoming[..size];
            if response == RESPONSE {
                found.entry(*address.ip()).or_insert(None);
            } else if let Some(json) = response.strip_prefix(NAMED_RESPONSE)
                && let Ok(advertisement) = serde_json::from_slice::<Advertisement>(json)
                && valid_name(&advertisement.name)
            {
                found.insert(*address.ip(), Some(advertisement));
            }
        }
    }
    Ok(found
        .into_iter()
        .map(|(address, advertisement)| Receiver {
            address,
            advertisement,
        })
        .collect())
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
