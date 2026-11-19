use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const TCP_PORT: u16 = 46387;
pub const DISCOVERY_PORT: u16 = 46388;
pub const MAX_MOVEMENT: i16 = 8_192;
pub type Token = [u8; 32];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
#[repr(u8)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

impl Edge {
    fn decode(value: u8) -> Result<Self> {
        match value {
            0 => Ok(Self::Left),
            1 => Ok(Self::Right),
            2 => Ok(Self::Top),
            3 => Ok(Self::Bottom),
            _ => bail!("Invalid edge"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    Move(i16, i16),
    Button(u8, bool),
    Wheel(i16, bool),
    Key {
        scan: u8,
        extended: bool,
        down: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame {
    Hello(Token),
    Ready,
    Enter(Edge, u16),
    Input(Input),
    Return(u16),
    Release,
    Ping(u32),
    Pong(u32),
}

const PAYLOAD_SIZES: [usize; 11] = [32, 0, 3, 4, 2, 3, 3, 2, 0, 4, 4];

impl Frame {
    pub async fn send(self, wire: &mut (impl AsyncWrite + Unpin)) -> Result<()> {
        let mut buf = [0; 33];
        buf[0] = match self {
            Self::Hello(token) => {
                buf[1..].copy_from_slice(&token);
                0
            }
            Self::Ready => 1,
            Self::Enter(edge, along) => {
                buf[1] = edge as u8;
                buf[2..4].copy_from_slice(&along.to_be_bytes());
                2
            }
            Self::Input(Input::Move(dx, dy)) => {
                buf[1..3].copy_from_slice(&dx.to_be_bytes());
                buf[3..5].copy_from_slice(&dy.to_be_bytes());
                3
            }
            Self::Input(Input::Button(button, down)) => {
                buf[1] = button;
                buf[2] = down.into();
                4
            }
            Self::Input(Input::Wheel(delta, horizontal)) => {
                buf[1..3].copy_from_slice(&delta.to_be_bytes());
                buf[3] = horizontal.into();
                5
            }
            Self::Input(Input::Key {
                scan,
                extended,
                down,
            }) => {
                buf[1] = scan;
                buf[2] = extended.into();
                buf[3] = down.into();
                6
            }
            Self::Return(along) => {
                buf[1..3].copy_from_slice(&along.to_be_bytes());
                7
            }
            Self::Release => 8,
            Self::Ping(id) | Self::Pong(id) => {
                buf[1..5].copy_from_slice(&id.to_be_bytes());
                if matches!(self, Self::Ping(_)) { 9 } else { 10 }
            }
        };
        wire.write_all(&buf[..1 + PAYLOAD_SIZES[buf[0] as usize]])
            .await?;
        wire.flush().await?;
        Ok(())
    }

    // Keep this future alive until it completes. Cancelling a partial read loses framing.
    pub async fn read(wire: &mut (impl AsyncRead + Unpin)) -> Result<Self> {
        let tag = wire.read_u8().await? as usize;
        let size = *PAYLOAD_SIZES
            .get(tag)
            .ok_or_else(|| anyhow::anyhow!("Unknown frame"))?;
        let mut buf = [0; 32];
        wire.read_exact(&mut buf[..size]).await?;
        let u16_at = |i| u16::from_be_bytes([buf[i], buf[i + 1]]);
        let i16_at = |i| u16_at(i) as i16;
        let boolean = |i: usize| match buf[i] {
            0 => Ok(false),
            1 => Ok(true),
            _ => bail!("Invalid flag"),
        };
        let id = || u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
        Ok(match tag {
            0 => Self::Hello(buf),
            1 => Self::Ready,
            2 => Self::Enter(Edge::decode(buf[0])?, u16_at(1)),
            3 => {
                let (dx, dy) = (i16_at(0), i16_at(2));
                ensure!(
                    dx.unsigned_abs() <= MAX_MOVEMENT as u16
                        && dy.unsigned_abs() <= MAX_MOVEMENT as u16,
                    "Invalid movement"
                );
                Self::Input(Input::Move(dx, dy))
            }
            4 => {
                ensure!(buf[0] < 5, "Invalid mouse button");
                Self::Input(Input::Button(buf[0], boolean(1)?))
            }
            5 => Self::Input(Input::Wheel(i16_at(0), boolean(2)?)),
            6 => {
                ensure!(buf[0] != 0, "Invalid scan code");
                Self::Input(Input::Key {
                    scan: buf[0],
                    extended: boolean(1)?,
                    down: boolean(2)?,
                })
            }
            7 => Self::Return(u16_at(0)),
            8 => Self::Release,
            9 => Self::Ping(id()),
            10 => Self::Pong(id()),
            _ => unreachable!(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn round_trips_all_frames_in_order() -> Result<()> {
        let frames = [
            Frame::Hello([251; 32]),
            Frame::Ready,
            Frame::Enter(Edge::Right, 400),
            Frame::Input(Input::Move(-54, 1000)),
            Frame::Input(Input::Button(4, true)),
            Frame::Input(Input::Wheel(-120, true)),
            Frame::Input(Input::Key {
                scan: 29,
                extended: true,
                down: false,
            }),
            Frame::Return(470),
            Frame::Release,
            Frame::Ping(42),
            Frame::Pong(42),
        ];
        let (mut a, mut b) = tokio::io::duplex(128);
        for frame in frames {
            frame.send(&mut a).await?;
            assert_eq!(Frame::read(&mut b).await?, frame);
        }
        Ok(())
    }

    #[tokio::test]
    async fn rejects_invalid_truncated_and_oversized_frames() {
        for bytes in [
            vec![200],
            vec![2, 4, 0, 0],
            vec![3, 0xff],
            vec![3, 0x80, 0, 0, 0],
            vec![4, 5, 1],
            vec![4, 0, 2],
            vec![6, 0, 0, 0],
        ] {
            assert!(Frame::read(&mut bytes.as_slice()).await.is_err());
        }
    }
}
