use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use rustls::pki_types::ServerName;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, split},
    net::{TcpListener, TcpStream},
    task::JoinSet,
    time::{MissedTickBehavior, interval, sleep, timeout},
};
use tokio_rustls::{TlsAcceptor, TlsConnector};

use crate::{
    clipboard, discovery,
    native::{Capture, WindowsDesktop},
    pairing::{self, Invite},
    protocol::{Edge, Frame, TCP_PORT, Token},
    receiver::{Desktop, Receiver},
    service::Shared,
    session_health::{DEADLINE, Heartbeat, QueuedFrame},
};

const HANDSHAKE: Duration = Duration::from_secs(3);
const HEARTBEAT: Duration = Duration::from_millis(250);
const MAX_HANDSHAKES: usize = 8;
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(45);

struct ServerApproval {
    shared: Arc<Shared>,
    generation: u64,
    issued: Mutex<Option<Token>>,
}

pub async fn listen(shared: Arc<Shared>, generation: u64, share_clipboard: bool) -> Result<()> {
    let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, TCP_PORT))
        .await
        .context("Could not open the receiver port. Is EdgeBridge already running?")?;
    let (mut config, code, token) = pairing::create()?;
    let advertisement = discovery::Advertisement {
        name: discovery::computer_name(),
        pin: Invite::parse(&code)?.pin(),
    };
    config.alpn_protocols.push(pairing::APPROVAL_ALPN.to_vec());
    if share_clipboard {
        config.alpn_protocols.insert(0, clipboard::ALPN.to_vec());
    }
    let acceptor = TlsAcceptor::from(Arc::new(config));
    // Manual IPv4 connection remains available if UDP is occupied or disabled.
    let udp = discovery::socket().await.ok();
    let discovered = udp.is_some();
    let approval = Arc::new(ServerApproval {
        shared: Arc::clone(&shared),
        generation,
        issued: Mutex::new(None),
    });
    shared.update(generation, |status| {
        status.phase = "waiting";
        status.code = Some(code);
        status.address = discovery::suggested_address();
        status.message =
            (!discovered).then(|| "Discovery unavailable. Use the receiver IPv4 address.".into());
    });

    let connections = async {
        loop {
            let (mut wire, address) =
                accept_connections(&listener, &acceptor, token, Some(&approval)).await?;
            let side = match server_clipboard(&mut wire, &listener, &acceptor).await {
                Ok(side) => side,
                Err(_) => continue,
            };
            shared.update(generation, |status| {
                status.phase = "connected";
                status.peer = Some(address.ip().to_string());
                status.clipboard_active = side.is_some();
            });
            let result = clipboard::alongside(
                receive(wire, WindowsDesktop, |active| {
                    shared.update(generation, |status| status.forwarding = active);
                }),
                side,
                || shared.update(generation, |status| status.clipboard_active = false),
            )
            .await;
            shared.update(generation, |status| {
                status.phase = "waiting";
                status.peer = None;
                status.forwarding = false;
                status.clipboard_active = false;
                status.message = result.err().map(|err| err.to_string());
            });
        }
    };

    if let Some(socket) = udp {
        tokio::pin!(connections);
        tokio::select! {
            result = &mut connections => result,
            _ = discovery::serve(socket, advertisement, || shared.ready_to_pair(generation)) => {
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

type PeerStream = tokio_rustls::server::TlsStream<TcpStream>;
type ClientStream = tokio_rustls::client::TlsStream<TcpStream>;

async fn server_clipboard(
    wire: &mut PeerStream,
    listener: &TcpListener,
    acceptor: &TlsAcceptor,
) -> Result<Option<PeerStream>> {
    if wire.get_ref().1.alpn_protocol() != Some(clipboard::ALPN) {
        return Ok(None);
    }
    let mut channel_key = [0; 32];
    getrandom::fill(&mut channel_key).map_err(|err| anyhow::anyhow!("{err}"))?;
    timeout(HANDSHAKE, async {
        wire.write_all(&channel_key).await?;
        wire.flush().await
    })
    .await??;
    let side = timeout(HANDSHAKE, accept_peer(listener, acceptor, channel_key))
        .await
        .ok()
        .and_then(Result::ok)
        .map(|(wire, _)| wire);
    timeout(HANDSHAKE, async {
        wire.write_u8(u8::from(side.is_some())).await?;
        wire.flush().await
    })
    .await??;
    Ok(side)
}

async fn accept_peer(
    listener: &TcpListener,
    acceptor: &TlsAcceptor,
    token: Token,
) -> Result<(PeerStream, SocketAddr)> {
    accept_connections(listener, acceptor, token, None).await
}

async fn accept_connections(
    listener: &TcpListener,
    acceptor: &TlsAcceptor,
    token: Token,
    approval: Option<&Arc<ServerApproval>>,
) -> Result<(PeerStream, SocketAddr)> {
    let mut handshakes = JoinSet::new();
    loop {
        tokio::select! {
            biased;
            Some(result) = handshakes.join_next() => {
                if let Ok(Some((mut wire, address))) = result
                    && matches!(timeout(HEARTBEAT, Frame::Ready.send(&mut wire)).await, Ok(Ok(())))
                {
                    // Dropping the set cancels every other pending handshake. Only this peer gets Ready.
                    return Ok((wire, address));
                }
            }
            accepted = listener.accept(), if handshakes.len() < MAX_HANDSHAKES => {
                let (stream, address) = accepted?;
                let acceptor = acceptor.clone();
                let approval = approval.cloned();
                handshakes.spawn(async move {
                    let mut wire = timeout(HANDSHAKE, accept_secure(stream, &acceptor)).await.ok()?.ok()?;
                    if wire.get_ref().1.alpn_protocol() == Some(pairing::APPROVAL_ALPN) {
                        if let Some(approval) = approval {
                            let _ = timeout(APPROVAL_TIMEOUT, approve_connection(wire, &approval)).await;
                        }
                        return None;
                    }
                    let frame = timeout(HANDSHAKE, Frame::read(&mut wire)).await.ok()?.ok()?;
                    let Frame::Hello(candidate) = frame else { return None };
                    let valid = pairing::authenticate(candidate, token)
                        || approval.as_ref().is_some_and(|approval| {
                            approval.issued.lock().unwrap().is_some_and(|token| pairing::authenticate(candidate, token))
                        });
                    if !valid {
                        return None;
                    }
                    Some((wire, address))
                });
            }
        }
    }
}

async fn accept_secure(stream: TcpStream, acceptor: &TlsAcceptor) -> Result<PeerStream> {
    stream.set_nodelay(true)?;
    Ok(acceptor.accept(stream).await?)
}

async fn approve_connection(mut wire: PeerStream, approval: &ServerApproval) -> Result<()> {
    let name = timeout(HANDSHAKE, async {
        let length = usize::from(wire.read_u8().await?);
        ensure!((1..=48).contains(&length), "Invalid PC name");
        let mut bytes = vec![0; length];
        wire.read_exact(&mut bytes).await?;
        let name = String::from_utf8(bytes)?;
        ensure!(discovery::valid_name(&name), "Invalid PC name");
        Ok::<_, anyhow::Error>(name)
    })
    .await??;
    let comparison =
        wire.get_ref()
            .1
            .export_keying_material([0; 8], pairing::APPROVAL_EXPORTER, None)?;
    let (_prompt, decision) =
        approval
            .shared
            .prompt(approval.generation, name, pairing::confirmation(comparison))?;
    wire.write_u8(1).await?;
    wire.flush().await?;
    let remote = async {
        ensure!(wire.read_u8().await? == 1, "Pairing declined");
        Ok::<_, anyhow::Error>(())
    };
    let local = async {
        ensure!(decision.await?, "Pairing declined");
        Ok::<_, anyhow::Error>(())
    };
    tokio::try_join!(remote, local)?;
    let mut token = [0; 32];
    getrandom::fill(&mut token).map_err(|err| anyhow::anyhow!("{err}"))?;
    *approval.issued.lock().unwrap() = Some(token);
    wire.write_all(&token).await?;
    wire.flush().await?;
    Ok(())
}

pub async fn request_pairing(
    shared: Arc<Shared>,
    generation: u64,
    ip: Ipv4Addr,
    advertisement: discovery::Advertisement,
    edges: (Edge, Edge),
    share_clipboard: bool,
) -> Result<()> {
    let invite = request_invite(&shared, generation, (ip, TCP_PORT).into(), advertisement).await?;
    send(shared, generation, ip, invite, edges, share_clipboard).await
}

async fn request_invite(
    shared: &Shared,
    generation: u64,
    address: SocketAddr,
    advertisement: discovery::Advertisement,
) -> Result<Invite> {
    let pin = advertisement.pin;
    let mut config = pairing::client(pin)?;
    config.alpn_protocols = vec![pairing::APPROVAL_ALPN.to_vec()];
    let connector = TlsConnector::from(Arc::new(config));
    let mut wire = timeout(HANDSHAKE, async {
        let stream = TcpStream::connect(address).await?;
        stream.set_nodelay(true)?;
        let mut wire = connector
            .connect(ServerName::try_from("edgebridge.local")?, stream)
            .await?;
        let name = discovery::computer_name();
        wire.write_u8(name.len() as u8).await?;
        wire.write_all(name.as_bytes()).await?;
        wire.flush().await?;
        ensure!(wire.read_u8().await? == 1, "Pairing unavailable");
        Ok::<_, anyhow::Error>(wire)
    })
    .await
    .context("The other PC did not answer the pairing request")?
    .context("Pairing unavailable. Check that the receiver is ready and try again.")?;
    let comparison =
        wire.get_ref()
            .1
            .export_keying_material([0; 8], pairing::APPROVAL_EXPORTER, None)?;
    let token = timeout(APPROVAL_TIMEOUT, async {
        let (_prompt, decision) = shared.prompt(
            generation,
            advertisement.name,
            pairing::confirmation(comparison),
        )?;
        let accepted = tokio::select! {
            result = decision => result?,
            _ = wire.read_u8() => bail!("The other PC closed the pairing request."),
        };
        ensure!(accepted, "Pairing declined");
        wire.write_u8(1).await?;
        wire.flush().await?;
        let mut token = [0; 32];
        wire.read_exact(&mut token)
            .await
            .context("Pairing declined or expired on the other PC.")?;
        Ok::<_, anyhow::Error>(token)
    })
    .await
    .context("Pairing request expired")??;
    Ok(Invite::from_approval(pin, token))
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
    address: SocketAddr,
    invite: Invite,
    connector: &TlsConnector,
) -> Result<ClientStream> {
    connect_with_token(address, invite.token, connector).await
}

async fn connect_with_token(
    address: SocketAddr,
    token: Token,
    connector: &TlsConnector,
) -> Result<ClientStream> {
    let stream = TcpStream::connect(address)
        .await
        .context("Receiver unreachable")?;
    stream.set_nodelay(true)?;
    let mut stream = connector
        .connect(ServerName::try_from("edgebridge.local")?, stream)
        .await
        .context(
            "Receiver certificate does not match the pairing code, or TLS could not connect",
        )?;
    Frame::Hello(token).send(&mut stream).await?;
    ensure!(
        Frame::read(&mut stream).await? == Frame::Ready,
        "Receiver rejected the pairing code"
    );
    Ok(stream)
}

async fn connect_channels(
    address: SocketAddr,
    invite: Invite,
    connector: &TlsConnector,
) -> Result<(ClientStream, Option<ClientStream>)> {
    let mut wire = timeout(HANDSHAKE, connect(address, invite, connector)).await??;
    if wire.get_ref().1.alpn_protocol() != Some(clipboard::ALPN) {
        return Ok((wire, None));
    }
    let mut channel_key = [0; 32];
    timeout(HANDSHAKE, wire.read_exact(&mut channel_key)).await??;
    let side = timeout(
        HANDSHAKE,
        connect_with_token(address, channel_key, connector),
    )
    .await
    .ok()
    .and_then(Result::ok);
    let acknowledged = timeout(HANDSHAKE, wire.read_u8()).await??;
    ensure!(acknowledged <= 1, "Invalid clipboard channel state");
    Ok((wire, side.filter(|_| acknowledged == 1)))
}

pub async fn send(
    shared: Arc<Shared>,
    generation: u64,
    ip: Ipv4Addr,
    invite: Invite,
    edges: (Edge, Edge),
    share_clipboard: bool,
) -> Result<()> {
    let mut config = invite.client()?;
    if share_clipboard {
        config.alpn_protocols.insert(0, clipboard::ALPN.to_vec());
    }
    let connector = TlsConnector::from(Arc::new(config));
    loop {
        let result = match connect_channels((ip, TCP_PORT).into(), invite, &connector).await {
            Ok((wire, side)) => {
                let (capture, events) = Capture::start(edges.0, edges.1).await?;
                shared.update(generation, |status| {
                    status.phase = "connected";
                    status.peer = Some(ip.to_string());
                    status.message = None;
                    status.clipboard_active = side.is_some();
                });
                let result = clipboard::alongside(
                    drive(wire, &capture, events, &shared, generation),
                    side,
                    || shared.update(generation, |status| status.clipboard_active = false),
                )
                .await;
                drop(capture);
                result
            }
            Err(err) => Err(err),
        };
        shared.update(generation, |status| {
            status.phase = "reconnecting";
            status.forwarding = false;
            status.latency_ms = None;
            status.clipboard_active = false;
            status.peer = None;
            status.message = result.err().map(|err| err.to_string());
        });
        sleep(Duration::from_secs(1)).await;
    }
}

async fn drive(
    wire: impl AsyncRead + AsyncWrite + Unpin,
    capture: &Capture,
    mut events: tokio::sync::mpsc::Receiver<QueuedFrame>,
    shared: &Shared,
    generation: u64,
) -> Result<()> {
    let (mut read, mut write) = split(wire);
    let heartbeat = Mutex::new(Heartbeat::new(Instant::now()));
    let receiving = async {
        loop {
            match Frame::read(&mut read).await? {
                Frame::Return(along) => capture.return_to(along),
                Frame::Pong(id) => {
                    let latency = heartbeat.lock().unwrap().acknowledge(id, Instant::now());
                    if let Some(latency) = latency {
                        shared.update(generation, |status| status.latency_ms = Some(latency));
                    }
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
                    let probe = heartbeat.lock().unwrap().probe(Instant::now())?;
                    shared.update(generation, |status| status.forwarding = capture.forwarding());
                    let Some(probe) = probe else { continue };
                    probe
                }
                event = events.recv() => event.context("Input capture stopped")?.fresh(Instant::now())?,
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
#[path = "benchmark.rs"]
mod benchmark;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn discovered_pairing_issues_a_token_only_after_both_pcs_accept() -> Result<()> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let (mut config, code, manual_token) = pairing::create()?;
        config.alpn_protocols.push(pairing::APPROVAL_ALPN.to_vec());
        let pin = Invite::parse(&code)?.pin();
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let secondary = Arc::new(Shared::default());
        let primary = Shared::default();
        let approval = Arc::new(ServerApproval {
            shared: Arc::clone(&secondary),
            generation: 0,
            issued: Mutex::new(None),
        });
        let server = accept_connections(&listener, &acceptor, manual_token, Some(&approval));
        let client = async {
            for accepted in [false, true] {
                let invitation = request_invite(
                    &primary,
                    0,
                    address,
                    discovery::Advertisement {
                        name: "Secondary-PC".into(),
                        pin,
                    },
                );
                let approve = async {
                    let (local, remote) = loop {
                        if let (Some(local), Some(remote)) =
                            (primary.snapshot().approval, secondary.snapshot().approval)
                        {
                            break (local, remote);
                        }
                        sleep(Duration::from_millis(5)).await;
                    };
                    assert_eq!(local.comparison, remote.comparison);
                    assert_eq!(local.comparison.len(), 8);
                    primary.decide(local.id, true).map_err(anyhow::Error::msg)?;
                    assert!(approval.issued.lock().unwrap().is_none());
                    secondary
                        .decide(remote.id, accepted)
                        .map_err(anyhow::Error::msg)?;
                    Ok::<_, anyhow::Error>(())
                };
                let (invite, choice) = tokio::join!(invitation, approve);
                choice?;
                assert_eq!(invite.is_ok(), accepted);
                assert!(primary.snapshot().approval.is_none());
                if let Ok(invite) = invite {
                    assert!(!pairing::authenticate(invite.token, manual_token));
                    let connector = TlsConnector::from(Arc::new(invite.client()?));
                    return connect(address, invite, &connector).await;
                }
                assert!(approval.issued.lock().unwrap().is_none());
            }
            bail!("The accepted request did not authenticate")
        };
        timeout(HANDSHAKE, async { tokio::try_join!(server, client) }).await??;
        Ok(())
    }

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
                let result = async {
                    let mut wire = accept_secure(stream, &acceptor).await?;
                    ensure!(
                        matches!(Frame::read(&mut wire).await?, Frame::Hello(candidate)
                        if pairing::authenticate(candidate, token)),
                        "Pairing denied"
                    );
                    Ok::<_, anyhow::Error>(wire)
                }
                .await;
                assert_eq!(result.is_ok(), expected);
                if let Ok(mut wire) = result {
                    Frame::Ready.send(&mut wire).await?;
                }
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

    #[tokio::test]
    async fn accepts_an_authorized_peer_while_one_negotiation_is_pending() -> Result<()> {
        let (config, code, token) = pairing::create()?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let connector = TlsConnector::from(Arc::new(Invite::parse(&code)?.client()?));
        let pending = TcpStream::connect(address).await?;
        let server = accept_peer(&listener, &acceptor, token);
        let client = connect(address, Invite::parse(&code)?, &connector);
        let ((_, peer), connected_tls) = timeout(Duration::from_secs(2), async {
            tokio::try_join!(server, client)
        })
        .await??;
        assert_eq!(peer, connected_tls.get_ref().0.local_addr()?);
        assert_ne!(peer, pending.local_addr()?);
        Ok(())
    }

    #[tokio::test]
    async fn receiver_cleans_held_input_when_the_stream_ends() -> Result<()> {
        use crate::layout::Layout;
        use crate::protocol::Input;
        use crate::state::{Point, Screen};
        struct NoWindowsInput(Mutex<Vec<Input>>);
        impl Desktop for &NoWindowsInput {
            fn layout(&self) -> Arc<Layout> {
                Arc::new(Layout::single(Screen {
                    width: 1024,
                    height: 768,
                }))
            }
            fn move_to(&self, _: Point) -> Result<()> {
                Ok(())
            }
            fn input(&self, event: Input) -> Result<()> {
                self.0.lock().unwrap().push(event);
                Ok(())
            }
        }
        let (mut tx, rx) = tokio::io::duplex(128);
        let fake = NoWindowsInput(Mutex::new(Vec::new()));
        let client = async move {
            Frame::Enter(Edge::Right, 30_000).send(&mut tx).await?;
            Frame::Input(Input::Button(0, true)).send(&mut tx).await?;
            Frame::Ping(11).send(&mut tx).await?;
            assert_eq!(Frame::read(&mut tx).await?, Frame::Pong(11));
            Ok::<_, anyhow::Error>(())
        };
        let (client, server) = timeout(Duration::from_secs(2), async {
            tokio::join!(client, receive(rx, &fake, |_| {}))
        })
        .await?;
        client?;
        assert!(server.is_err());
        assert_eq!(
            *fake.0.lock().unwrap(),
            [Input::Button(0, true), Input::Button(0, false)]
        );
        Ok(())
    }

    #[tokio::test]
    async fn clipboard_channel_requires_both_peers_to_enable_it() -> Result<()> {
        for (receiver, primary, expected) in [
            (true, true, true),
            (false, true, false),
            (true, false, false),
        ] {
            let (mut config, code, token) = pairing::create()?;
            let invite = Invite::parse(&code)?;
            let mut client = invite.client()?;
            if receiver {
                config.alpn_protocols.insert(0, clipboard::ALPN.to_vec());
            }
            if primary {
                client.alpn_protocols.insert(0, clipboard::ALPN.to_vec());
            }
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
            let address = listener.local_addr()?;
            let acceptor = TlsAcceptor::from(Arc::new(config));
            let connector = TlsConnector::from(Arc::new(client));
            let server = async {
                let (mut main, _) = accept_peer(&listener, &acceptor, token).await?;
                let side = server_clipboard(&mut main, &listener, &acceptor).await?;
                assert_eq!(side.is_some(), expected);
                assert_eq!(Frame::read(&mut main).await?, Frame::Ping(4));
                Frame::Pong(4).send(&mut main).await?;
                Ok::<_, anyhow::Error>(())
            };
            let client = async {
                let (mut main, side) = connect_channels(address, invite, &connector).await?;
                assert_eq!(side.is_some(), expected);
                Frame::Ping(4).send(&mut main).await?;
                assert_eq!(Frame::read(&mut main).await?, Frame::Pong(4));
                Ok::<_, anyhow::Error>(())
            };
            timeout(Duration::from_secs(3), async {
                tokio::try_join!(server, client)
            })
            .await??;
        }
        Ok(())
    }
}
