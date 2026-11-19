use super::{accept_peer, connect, receive};
use crate::{
    pairing::{self, Invite},
    protocol::{Edge, Frame, Input},
    receiver::Desktop,
    state::{Point, Screen},
};
use anyhow::Result;
use std::{
    net::Ipv4Addr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::net::TcpListener;
use tokio_rustls::{TlsAcceptor, TlsConnector};

struct SimulatedDesktop;

impl Desktop for SimulatedDesktop {
    fn screen(&self) -> Screen {
        Screen {
            width: 1920,
            height: 1080,
        }
    }
    fn move_to(&self, _: Point) -> Result<()> {
        Ok(())
    }
    fn input(&self, _: Input) -> Result<()> {
        Ok(())
    }
}

fn report(name: &str, samples: &mut [Duration]) {
    samples.sort_unstable();
    let percentile = |percent: usize| {
        samples[(samples.len() * percent).div_ceil(100) - 1].as_secs_f64() * 1_000_000.0
    };
    println!(
        "{name}: n={} p50={:.1}us p95={:.1}us p99={:.1}us",
        samples.len(),
        percentile(50),
        percentile(95),
        percentile(99)
    );
}

#[tokio::test]
#[ignore = "measurement depends on the host and is run explicitly"]
async fn tls_loopback_profile() -> Result<()> {
    const RTT_SAMPLES: u32 = 1_000;
    const BURST_ROUNDS: u32 = 100;
    const EVENTS_PER_BURST: u32 = 64;
    let (config, code, token) = pairing::create()?;
    let invite = Invite::parse(&code)?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let connector = TlsConnector::from(Arc::new(invite.client()?));

    let server = async {
        let (wire, _) = accept_peer(&listener, &acceptor, token).await?;
        receive(wire, SimulatedDesktop, |_| {}).await
    };
    let client = async {
        let started = Instant::now();
        let mut wire = connect(address, invite, &connector).await?;
        println!(
            "TCP + pinned TLS + Ready: {:.1}us",
            started.elapsed().as_secs_f64() * 1_000_000.0
        );

        let mut samples = Vec::new();
        for id in 0..(RTT_SAMPLES + 100) {
            let start = Instant::now();
            Frame::Ping(id).send(&mut wire).await?;
            assert_eq!(Frame::read(&mut wire).await?, Frame::Pong(id));
            if id >= 100 {
                samples.push(start.elapsed());
            }
        }
        report("Idle Ping/Pong loopback RTT", &mut samples);
        Frame::Enter(Edge::Right, 32_000).send(&mut wire).await?;

        let mut bursts = Vec::new();
        for id in 0..BURST_ROUNDS {
            let start = Instant::now();
            for _ in 0..EVENTS_PER_BURST {
                Frame::Input(Input::Move(-1, 0)).send(&mut wire).await?;
            }
            Frame::Ping(id).send(&mut wire).await?;
            assert_eq!(Frame::read(&mut wire).await?, Frame::Pong(id));
            bursts.push(start.elapsed());
        }
        report("64 ordered movements plus acknowledgement", &mut bursts);
        Ok::<_, anyhow::Error>(())
    };
    let (client, server) = tokio::time::timeout(Duration::from_secs(15), async {
        tokio::join!(client, server)
    })
    .await?;
    client?;
    assert!(
        server.is_err(),
        "Receiver should exit after the test client closes"
    );
    Ok(())
}
