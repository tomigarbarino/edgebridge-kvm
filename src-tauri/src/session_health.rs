use std::time::{Duration, Instant};

use anyhow::{Result, ensure};

use crate::protocol::Frame;

pub const DEADLINE: Duration = Duration::from_millis(1_250);
const MAX_INPUT_AGE: Duration = Duration::from_millis(200);

pub struct Heartbeat {
    last_reply: Instant,
    next_id: u32,
    pending: Option<(u32, Instant)>,
}

impl Heartbeat {
    pub fn new(now: Instant) -> Self {
        Self {
            last_reply: now,
            next_id: 0,
            pending: None,
        }
    }

    pub fn probe(&mut self, now: Instant) -> Result<Option<Frame>> {
        ensure!(
            now.duration_since(self.last_reply) < DEADLINE,
            "Receiver timed out"
        );
        if self.pending.is_some() {
            return Ok(None);
        }
        let id = self.next_id;
        self.next_id = id.wrapping_add(1);
        self.pending = Some((id, now));
        Ok(Some(Frame::Ping(id)))
    }

    pub fn acknowledge(&mut self, id: u32, now: Instant) -> Option<u32> {
        let (expected, sent_at) = self.pending?;
        if id != expected || now.duration_since(self.last_reply) >= DEADLINE {
            return None;
        }
        self.pending = None;
        self.last_reply = now;
        Some(now.duration_since(sent_at).as_millis() as u32)
    }
}

pub struct QueuedFrame {
    frame: Frame,
    captured_at: Instant,
}

impl QueuedFrame {
    pub fn new(frame: Frame, captured_at: Instant) -> Self {
        Self { frame, captured_at }
    }

    pub fn fresh(self, now: Instant) -> Result<Frame> {
        ensure!(
            now.duration_since(self.captured_at) < MAX_INPUT_AGE,
            "Input queue was delayed. Local control restored."
        );
        Ok(self.frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn late_or_unmatched_replies_do_not_extend_leases() -> Result<()> {
        let now = Instant::now();
        let mut heart = Heartbeat::new(now);
        assert_eq!(heart.acknowledge(0, now), None);
        assert_eq!(heart.probe(now)?, Some(Frame::Ping(0)));
        assert_eq!(heart.probe(now + Duration::from_millis(250))?, None);
        assert_eq!(heart.acknowledge(1, now + Duration::from_secs(1)), None);
        assert_eq!(heart.acknowledge(0, now + DEADLINE), None);
        assert!(heart.probe(now + DEADLINE).is_err());
        Ok(())
    }

    #[test]
    fn matched_reply_reports_latency_and_advances_the_lease() -> Result<()> {
        let now = Instant::now();
        let reply = now + Duration::from_millis(41);
        let mut heart = Heartbeat::new(now);
        assert_eq!(heart.probe(now)?, Some(Frame::Ping(0)));
        assert_eq!(heart.acknowledge(0, reply), Some(41));
        assert_eq!(heart.acknowledge(0, reply), None);
        assert_eq!(heart.probe(now + DEADLINE)?, Some(Frame::Ping(1)));
        Ok(())
    }

    #[test]
    fn stale_input_rejects_the_session_instead_of_silently_dropping_a_frame() -> Result<()> {
        let now = Instant::now();
        assert_eq!(
            QueuedFrame::new(Frame::Release, now).fresh(now)?,
            Frame::Release
        );
        assert!(
            QueuedFrame::new(Frame::Release, now)
                .fresh(now + MAX_INPUT_AGE)
                .is_err()
        );
        Ok(())
    }
}
