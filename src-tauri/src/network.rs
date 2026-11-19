use std::{net::Ipv4Addr, sync::Arc, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use rustls::pki_types::ServerName;
use tokio::{
    io::{AsyncRead, AsyncWrite, split},
    net::{TcpListener, TcpStream},
    time::{Instant, MissedTickBehavior, interval, sleep, timeout},
};
use tokio_rustls::{TlsAcceptor, TlsConnector};

use crate::{
    discovery,
    native::{Capture, WindowsDesktop},
    pairing::{self, Invite},
    protocol::{Edge, Frame, TCP_PORT, Token},
    receiver::{Desktop, Receiver},
    service::Shared,
};

const HANDSHAKE: Duration = Duration::from_secs(3);
const HEARTBEAT: Duration = Duration::from_millis(250);
const DEADLINE: Duration = Duration::from_millis(1_250);

pub async fn listen(shared: Arc<Shared>, generation: u64) -> Result<()> {
    let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, TCP_PORT))
        .await
        .context("Could not open the receiver port. Is EdgeBridge already running?")?;
    let (config, code, token) = pairing::create()?;
    let acceptor = TlsAcceptor::from(Arc::new(config));
    // Manual IPv4 connection remains available if UDP is occupied or disabled.
    let udp = discovery::socket().await.ok();
    let discovered = udp.is_some();
    shared.update(generation, |status| {
        status.phase = "waiting";
        status.code = Some(code);
        status.address = discovery::suggested_address();
        status.message =
            (!discovered).then(|| "Discovery unavailable. Use the receiver IPv4 address.".into());
    });

    let connections = async {
        loop {
            let (stream, address) = listener.accept().await?;
            let wire = timeout(HANDSHAKE, authorize(stream, &acceptor, token)).await;
            if let Ok(Ok(wire)) = wire {
                shared.update(generation, |status| {
                    status.phase = "connected";
                    status.peer = Some(address.ip().to_string());
                });
                let result = receive(wire, WindowsDesktop, |active| {
                    shared.update(generation, |status| status.forwarding = active);
                })
                .await;
                shared.update(generation, |status| {
                    status.phase = "waiting";
                    status.peer = None;
                    status.forwarding = false;
                    status.message = result.err().map(|err| err.to_string());
                });
            }
        }
    };

    if let Some(socket) = udp {
        tokio::pin!(connections);
        tokio::select! {
            result = &mut connections => result,
            _ = discovery::serve(socket) => {
                shared.update(generation, |status| {
                    status.message = Some("Discovery stopped. Use the receiver IPv4 address.".into());
                });
                connections.await
            }
        }
    } else {
        connections.await
    }
}

async fn authorize(
    stream: TcpStream,
    acceptor: &TlsAcceptor,
    token: Token,
) -> Result<tokio_rustls::server::TlsStream<TcpStream>> {
    stream.set_nodelay(true)?;
    let mut stream = acceptor.accept(stream).await?;
    ensure!(
        matches!(Frame::read(&mut stream).await?, Frame::Hello(candidate)
        if pairing::authenticate(candidate, token)),
        "Pairing denied"
    );
    Frame::Ready.send(&mut stream).await?;
    Ok(stream)
}

async fn receive(
    mut wire: impl AsyncRead + AsyncWrite + Unpin,
    desktop: impl Desktop,
    mut active: impl FnMut(bool),
) -> Result<()> {
    let mut state = Receiver::new(desktop);
    loop {
        let frame = timeout(DEADLINE, Frame::read(&mut wire))
            .await
            .context("Sender timed out")??;
        match frame {
            Frame::Enter(edge, along) => {
                state.enter(edge, along)?;
                active(true);
            }
            Frame::Input(input) => {
                if let Some(frame) = state.input(input)? {
                    active(false);
                    timeout(HEARTBEAT, frame.send(&mut wire)).await??;
                }
            }
            Frame::Release => {
                state.release();
                active(false);
            }
            Frame::Ping(id) => timeout(HEARTBEAT, Frame::Pong(id).send(&mut wire)).await??,
            _ => bail!("Unexpected sender frame"),
        }
    }
}

async fn connect(
    ip: Ipv4Addr,
    invite: Invite,
    connector: &TlsConnector,
) -> Result<tokio_rustls::client::TlsStream<TcpStream>> {
    let stream = TcpStream::connect((ip, TCP_PORT))
        .await
        .context("Receiver unreachable")?;
    stream.set_nodelay(true)?;
    let mut stream = connector
        .connect(ServerName::try_from("edgebridge.local")?, stream)
        .await
        .context(
            "Receiver certificate does not match the pairing code, or TLS could not connect",
        )?;
    Frame::Hello(invite.token).send(&mut stream).await?;
    ensure!(
        Frame::read(&mut stream).await? == Frame::Ready,
        "Receiver rejected the pairing code"
    );
    Ok(stream)
}

pub async fn send(
    shared: Arc<Shared>,
    generation: u64,
    ip: Ipv4Addr,
    invite: Invite,
    from: Edge,
    to: Edge,
) -> Result<()> {
    let connector = TlsConnector::from(Arc::new(invite.client()?));
    loop {
        let result = match timeout(HANDSHAKE, connect(ip, invite, &connector)).await {
            Ok(Ok(wire)) => {
                let (capture, events) = Capture::start(from, to).await?;
                shared.update(generation, |status| {
                    status.phase = "connected";
                    status.peer = Some(ip.to_string());
                    status.message = None;
                });
                let result = drive(wire, &capture, events, &shared, generation).await;
                drop(capture);
                result
            }
            Ok(Err(err)) => Err(err),
            Err(_) => Err(anyhow::anyhow!("Connection timed out")),
        };
        shared.update(generation, |status| {
            status.phase = "reconnecting";
            status.forwarding = false;
            status.latency_ms = None;
            status.peer = None;
            status.message = result.err().map(|err| err.to_string());
        });
        sleep(Duration::from_secs(1)).await;
    }
}

async fn drive(
    wire: impl AsyncRead + AsyncWrite + Unpin,
    capture: &Capture,
    mut events: tokio::sync::mpsc::Receiver<Frame>,
    shared: &Shared,
    generation: u64,
) -> Result<()> {
    let (mut read, mut write) = split(wire);
    let last_pong = std::sync::Mutex::new(Instant::now());
    let start = Instant::now();
    let receiving = async {
        loop {
            match Frame::read(&mut read).await? {
                Frame::Return(along) => capture.return_to(along),
                Frame::Pong(id) => {
                    let now = Instant::now();
                    *last_pong.lock().unwrap() = now;
                    let latency = (now.duration_since(start).as_millis() as u32).wrapping_sub(id);
                    shared.update(generation, |status| status.latency_ms = Some(latency));
                }
                _ => bail!("Unexpected receiver frame"),
            }
        }
    };
    let sending = async {
        let mut ticker = interval(HEARTBEAT);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            let frame = tokio::select! {
                biased;
                _ = ticker.tick() => {
                    ensure!(last_pong.lock().unwrap().elapsed() < DEADLINE, "Receiver timed out");
                    shared.update(generation, |status| status.forwarding = capture.forwarding());
                    Frame::Ping(start.elapsed().as_millis() as u32)
                }
                frame = events.recv() => frame.context("Input capture stopped")?,
            };
            ensure!(capture.alive(), "Input capture stopped");
            timeout(HEARTBEAT, frame.send(&mut write))
                .await
                .context("Receiver stopped reading")??;
        }
    };
    let ((), ()) = tokio::try_join!(receiving, sending)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn authorizes_reconnects_and_rejects_wrong_token_and_cert() -> Result<()> {
        let localhost = Ipv4Addr::LOCALHOST;
        let listener = TcpListener::bind((localhost, 0)).await?;
        let address = listener.local_addr()?;
        let (server, code, token) = pairing::create()?;
        let correct = Invite::parse(&code)?;
        let wrong = Invite::parse(&pairing::create()?.1)?;
        let acceptor = TlsAcceptor::from(Arc::new(server));
        let server = async {
            for expected in [true, true, false, false] {
                let (stream, _) = listener.accept().await?;
                assert_eq!(authorize(stream, &acceptor, token).await.is_ok(), expected);
            }
            Ok::<_, anyhow::Error>(())
        };
        let client = async {
            for (invite, token, expected) in [
                (correct, token, true),
                (correct, token, true),
                (correct, wrong.token, false),
                (wrong, token, false),
            ] {
                let connector = TlsConnector::from(Arc::new(invite.client()?));
                let tcp = TcpStream::connect(address).await?;
                let result = connector
                    .connect(ServerName::try_from("edgebridge.local")?, tcp)
                    .await;
                if let Ok(mut stream) = result {
                    Frame::Hello(token).send(&mut stream).await?;
                    assert_eq!(
                        Frame::read(&mut stream).await.ok(),
                        expected.then_some(Frame::Ready)
                    );
                } else {
                    assert!(!expected);
                }
            }
            Ok::<_, anyhow::Error>(())
        };
        timeout(Duration::from_secs(4), async {
            tokio::try_join!(server, client)
        })
        .await??;
        Ok(())
    }
}
