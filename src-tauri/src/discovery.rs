use std::{
    collections::BTreeSet,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    time::Duration,
};

use anyhow::Result;
use tokio::{
    net::UdpSocket,
    time::{Instant, timeout_at},
};

use crate::protocol::DISCOVERY_PORT;

const QUERY: &[u8] = b"EdgeBridge/1 find";
const RESPONSE: &[u8] = b"EdgeBridge/1 receiver";

pub async fn socket() -> Result<UdpSocket> {
    Ok(UdpSocket::bind((Ipv4Addr::UNSPECIFIED, DISCOVERY_PORT)).await?)
}

pub async fn serve(socket: UdpSocket) -> Result<()> {
    let mut incoming = [0; 64];
    loop {
        let (size, address) = socket.recv_from(&mut incoming).await?;
        if &incoming[..size] == QUERY {
            socket.send_to(RESPONSE, address).await?;
        }
    }
}

pub async fn scan() -> Result<Vec<String>> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
    socket.set_broadcast(true)?;
    socket
        .send_to(
            QUERY,
            SocketAddrV4::new(Ipv4Addr::BROADCAST, DISCOVERY_PORT),
        )
        .await?;
    let deadline = Instant::now() + Duration::from_millis(850);
    let mut incoming = [0; 64];
    let mut found = BTreeSet::new();
    while let Ok(reply) = timeout_at(deadline, socket.recv_from(&mut incoming)).await {
        let (size, address) = reply?;
        if let SocketAddr::V4(address) = address
            && &incoming[..size] == RESPONSE
        {
            found.insert(*address.ip());
        }
        if found.len() >= 32 {
            break;
        }
    }
    Ok(found.into_iter().map(|ip| ip.to_string()).collect())
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
        let server = tokio::spawn(serve(listener));
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
            Ok(())
        }
        .await;
        server.abort();
        result
    }
}
