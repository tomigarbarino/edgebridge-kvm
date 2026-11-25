use anyhow::{Context, Result, bail, ensure};
use clipboard_win::{formats, get_clipboard, raw, set_clipboard};
use std::{os::windows::fs::MetadataExt, path::Path, sync::Mutex, time::Duration};
use tempfile::TempDir;
use tokio::{
    fs,
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, split},
    sync::watch,
    time::{interval, timeout},
};

pub const ALPN: &[u8] = b"edgebridge-kvm/2-clipboard";
pub const LIVE_ALPN: &[u8] = b"edgebridge-kvm/4-live-clipboard";
const PREFERENCE: u8 = 0;
const MAX_PAYLOAD: usize = 64 * 1024 * 1024;
const MAX_FILES: usize = 32;
const MAX_NAME: usize = 255;
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Text = 1,
    Image = 2,
    Files = 3,
}

impl TryFrom<u8> for Kind {
    type Error = anyhow::Error;
    fn try_from(value: u8) -> Result<Self> {
        match value {
            1 => Ok(Self::Text),
            2 => Ok(Self::Image),
            3 => Ok(Self::Files),
            _ => bail!("Unsupported clipboard format"),
        }
    }
}

enum LocalContent {
    Inline(Kind, Vec<u8>),
    Files(Vec<String>),
}

struct DesktopClipboard {
    sequence: Option<std::num::NonZeroU32>,
    files: Option<TempDir>,
}

pub async fn alongside(
    control: impl Future<Output = Result<()>>,
    clipboard: Option<impl AsyncRead + AsyncWrite + Unpin>,
    local: Option<watch::Receiver<bool>>,
    active: impl Fn(bool),
    disabled: impl FnOnce(),
) -> Result<()> {
    let Some(wire) = clipboard else {
        return control.await;
    };
    tokio::pin!(control);
    tokio::select! {
        result = &mut control => result,
        _ = exchange(wire, local, active) => {
            disabled();
            control.await
        }
    }
}

struct Permissions {
    local: watch::Receiver<bool>,
    remote: watch::Receiver<bool>,
}

impl Permissions {
    fn allowed(&self) -> bool {
        *self.local.borrow() && *self.remote.borrow()
    }
}

async fn exchange(
    wire: impl AsyncRead + AsyncWrite + Unpin,
    local: Option<watch::Receiver<bool>>,
    active: impl Fn(bool),
) -> Result<()> {
    let clipboard = Mutex::new(DesktopClipboard {
        sequence: raw::seq_num(),
        files: None,
    });
    let (read, write) = split(wire);
    let (remote, peer) = watch::channel(false);
    let permissions = local.map(|local| Permissions {
        local,
        remote: peer,
    });
    tokio::try_join!(
        send_changes(write, &clipboard, permissions.as_ref(), &active),
        receive_changes(read, &clipboard, permissions.as_ref(), remote)
    )?;
    Ok(())
}

async fn send_changes(
    mut write: impl AsyncWrite + Unpin,
    clipboard: &Mutex<DesktopClipboard>,
    permissions: Option<&Permissions>,
    active: &impl Fn(bool),
) -> Result<()> {
    let mut local = permissions.map(|p| p.local.clone());
    let mut remote = permissions.map(|p| p.remote.clone());
    if let Some(local) = &mut local {
        let enabled = *local.borrow_and_update();
        write_preference(&mut write, enabled).await?;
    }
    let mut ticker = interval(Duration::from_millis(120));
    loop {
        if let (Some(local), Some(remote)) = (&mut local, &mut remote) {
            tokio::select! {
                biased;
                result = local.changed() => {
                    result?;
                    let enabled = *local.borrow_and_update();
                    clipboard.lock().unwrap().sequence = raw::seq_num();
                    active(enabled && *remote.borrow());
                    write_preference(&mut write, enabled).await?;
                    continue;
                }
                result = remote.changed() => {
                    result?;
                    let enabled = *remote.borrow_and_update();
                    clipboard.lock().unwrap().sequence = raw::seq_num();
                    active(enabled && *local.borrow());
                    continue;
                }
                _ = ticker.tick() => {}
            }
        } else {
            ticker.tick().await;
        }
        if permissions.is_some_and(|p| !p.allowed()) {
            continue;
        }
        let content = {
            let mut local = clipboard.lock().unwrap();
            let sequence = raw::seq_num();
            if sequence == local.sequence {
                continue;
            }
            local.sequence = sequence;
            // Keep incoming temporary files available for Explorer while the session is open.
            capture()
        };
        let (kind, bytes) = match content {
            Ok(Some(LocalContent::Inline(kind, bytes))) => (kind, bytes),
            Ok(Some(LocalContent::Files(paths))) => match encode_files(paths).await {
                Ok(bytes) => (Kind::Files, bytes),
                Err(_) => continue,
            },
            _ => continue,
        };
        if bytes.len() > MAX_PAYLOAD || permissions.is_some_and(|p| !p.allowed()) {
            continue;
        }
        timeout(TRANSFER_TIMEOUT, async {
            write.write_u8(kind as u8).await?;
            write.write_u32(bytes.len() as u32).await?;
            write.write_all(&bytes).await?;
            write.flush().await
        })
        .await??;
    }
}

async fn write_preference(write: &mut (impl AsyncWrite + Unpin), enabled: bool) -> Result<()> {
    timeout(TRANSFER_TIMEOUT, async {
        write.write_all(&[PREFERENCE, u8::from(enabled)]).await?;
        write.flush().await
    })
    .await??;
    Ok(())
}

async fn receive_changes(
    mut read: impl AsyncRead + Unpin,
    clipboard: &Mutex<DesktopClipboard>,
    permissions: Option<&Permissions>,
    remote: watch::Sender<bool>,
) -> Result<()> {
    loop {
        // An idle clipboard channel is normal. Start the transfer deadline after the tag.
        let tag = read.read_u8().await?;
        if tag == PREFERENCE && permissions.is_some() {
            let value = timeout(TRANSFER_TIMEOUT, read.read_u8()).await??;
            ensure!(value <= 1, "Invalid clipboard preference");
            remote.send_if_modified(|enabled| {
                let changed = *enabled != (value == 1);
                *enabled = value == 1;
                changed
            });
            continue;
        }
        let kind = Kind::try_from(tag)?;
        let bytes = timeout(TRANSFER_TIMEOUT, async {
            let size = read.read_u32().await? as usize;
            ensure!(
                size <= MAX_PAYLOAD,
                "Clipboard transfer exceeds the size limit"
            );
            if permissions.is_some_and(|p| !p.allowed()) {
                let size = size as u64;
                let consumed =
                    tokio::io::copy(&mut (&mut read).take(size), &mut tokio::io::sink()).await?;
                ensure!(consumed == size, "Incomplete clipboard transfer");
                return Ok(None);
            }
            let mut bytes = vec![0; size];
            read.read_exact(&mut bytes).await?;
            Ok::<_, anyhow::Error>(Some(bytes))
        })
        .await??;
        let Some(bytes) = bytes else { continue };
        let files = if kind == Kind::Files {
            Some(stage_files(&bytes).await?)
        } else {
            None
        };
        // Hold the preference through the final OS write so disabling and
        // applying received data are ordered even while a transfer is in flight.
        let enabled = permissions.map(|p| p.local.borrow());
        if enabled.as_ref().is_some_and(|value| !**value) {
            continue;
        }
        let mut local = clipboard.lock().unwrap();
        match kind {
            Kind::Text => {
                let text = std::str::from_utf8(&bytes)?;
                ensure!(!text.contains('\0'), "Invalid clipboard text");
                set_clipboard(formats::Unicode, text).map_err(|e| anyhow::anyhow!("{e}"))?;
            }
            Kind::Image => {
                validate_image(&bytes)?;
                set_clipboard(formats::RawData(formats::CF_DIB), bytes.as_slice())
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
            }
            Kind::Files => {
                let (dir, paths) = files.context("Files were not staged")?;
                let _opened = clipboard_win::Clipboard::new_attempts(10)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                raw::set_file_list(&paths).map_err(|e| anyhow::anyhow!("{e}"))?;
                local.files = Some(dir);
            }
        }
        local.sequence = raw::seq_num();
    }
}

fn capture() -> Result<Option<LocalContent>> {
    use clipboard_win::formats::Format;
    if formats::FileList.is_format_avail() {
        let paths: Vec<String> =
            get_clipboard(formats::FileList).map_err(|e| anyhow::anyhow!("{e}"))?;
        return Ok(Some(LocalContent::Files(paths)));
    }
    let dib = formats::RawData(formats::CF_DIB);
    if dib.is_format_avail() {
        let bytes: Vec<u8> = get_clipboard(dib).map_err(|e| anyhow::anyhow!("{e}"))?;
        validate_image(&bytes)?;
        return Ok(Some(LocalContent::Inline(Kind::Image, bytes)));
    }
    if formats::Unicode.is_format_avail() {
        let text: String = get_clipboard(formats::Unicode).map_err(|e| anyhow::anyhow!("{e}"))?;
        return Ok(Some(LocalContent::Inline(Kind::Text, text.into_bytes())));
    }
    Ok(None)
}

fn validate_image(bytes: &[u8]) -> Result<()> {
    ensure!(
        bytes.len() >= 40 && bytes.len() <= MAX_PAYLOAD,
        "Invalid clipboard image length"
    );
    let word = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    let header = word(0) as usize;
    let width = word(4) as i32;
    let height = (word(8) as i32).unsigned_abs() as usize;
    let bits = u16::from_le_bytes([bytes[14], bytes[15]]) as usize;
    let compression = word(16);
    ensure!(
        matches!(header, 40 | 108 | 124)
            && (1..=4096).contains(&width)
            && (1..=4096).contains(&height)
            && bytes[12..14] == [1, 0]
            && matches!(bits, 24 | 32)
            && word(32) == 0
            && (compression == 0 || compression == 3 && bits == 32),
        "Unsupported bitmap layout"
    );
    // BITMAPV5 profile offsets are not needed for a clipboard image.
    if header == 124 {
        ensure!(
            bytes.len() >= 124 && word(112) == 0 && word(116) == 0,
            "Unsupported bitmap profile"
        );
    }
    let masks = if header == 40 && compression == 3 {
        12
    } else {
        0
    };
    let stride = (width as usize * bits).div_ceil(32) * 4;
    ensure!(
        header + masks + stride * height <= bytes.len(),
        "Truncated bitmap"
    );
    Ok(())
}

fn safe_name(name: &str) -> bool {
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_ascii_uppercase();
    !name.is_empty()
        && name.len() <= MAX_NAME
        && !name.ends_with([' ', '.'])
        && !name
            .chars()
            .any(|c| c.is_control() || r#"\/:"<>|?*"#.contains(c))
        && !matches!(
            stem.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        )
        && !["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|suffix| {
                matches!(
                    suffix,
                    "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "Â¹" | "Â²" | "Â³"
                )
            })
        })
}

async fn encode_files(paths: Vec<String>) -> Result<Vec<u8>> {
    ensure!(
        (1..=MAX_FILES).contains(&paths.len()),
        "Too many clipboard files"
    );
    let mut bytes = vec![paths.len() as u8];
    for path in paths {
        let path = Path::new(&path);
        ensure!(
            matches!(path.components().next(), Some(std::path::Component::Prefix(prefix))
                if matches!(prefix.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)))
                && path.is_absolute(),
            "Clipboard files must be local absolute paths"
        );
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .context("Invalid filename")?;
        ensure!(safe_name(name), "Unsupported filename");
        let metadata = fs::symlink_metadata(path).await?;
        ensure!(
            metadata.is_file() && metadata.file_attributes() & 0x400 == 0,
            "Only ordinary files are supported"
        );
        let header = name.len() + 6;
        let remaining = MAX_PAYLOAD.saturating_sub(bytes.len() + header);
        ensure!(
            bytes.len() + header <= MAX_PAYLOAD && metadata.len() <= remaining as u64,
            "Clipboard files exceed the size limit"
        );
        bytes.extend_from_slice(&(name.len() as u16).to_be_bytes());
        bytes.extend_from_slice(name.as_bytes());
        let size_at = bytes.len();
        bytes.extend_from_slice(&[0; 4]);
        let file = fs::File::open(path).await?;
        let read = file
            .take(remaining as u64 + 1)
            .read_to_end(&mut bytes)
            .await?;
        ensure!(read <= remaining, "Clipboard file changed size");
        bytes[size_at..size_at + 4].copy_from_slice(&(read as u32).to_be_bytes());
    }
    Ok(bytes)
}

fn parse_files(bytes: &[u8]) -> Result<Vec<(&str, &[u8])>> {
    fn take<'a>(buffer: &mut &'a [u8], n: usize) -> Result<&'a [u8]> {
        let (head, rest) = buffer
            .split_at_checked(n)
            .context("Truncated clipboard files")?;
        *buffer = rest;
        Ok(head)
    }

    let mut cursor = bytes;
    let count = take(&mut cursor, 1)?[0] as usize;
    ensure!(
        (1..=MAX_FILES).contains(&count),
        "Invalid clipboard file count"
    );
    let mut files = Vec::with_capacity(count);
    for _ in 0..count {
        let len = u16::from_be_bytes(take(&mut cursor, 2)?.try_into()?) as usize;
        let name = std::str::from_utf8(take(&mut cursor, len)?)?;
        ensure!(safe_name(name), "Invalid received filename");
        let size = u32::from_be_bytes(take(&mut cursor, 4)?.try_into()?) as usize;
        files.push((name, take(&mut cursor, size)?));
    }
    ensure!(cursor.is_empty(), "Trailing clipboard bytes");
    Ok(files)
}

async fn stage_files(bytes: &[u8]) -> Result<(TempDir, Vec<String>)> {
    let files = parse_files(bytes)?;
    let dir = tempfile::Builder::new()
        .prefix("edgebridge-clipboard-")
        .tempdir()?;
    let mut paths = Vec::with_capacity(files.len());
    for (index, (name, body)) in files.into_iter().enumerate() {
        let parent = dir.path().join(index.to_string());
        fs::create_dir(&parent).await?;
        let path = parent.join(name);
        fs::write(&path, body).await?;
        paths.push(
            path.to_str()
                .context("Invalid destination path")?
                .to_owned(),
        );
    }
    Ok((dir, paths))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn live_clipboard_changes_preference_without_reopening_the_stream() -> Result<()> {
        let (mut peer, wire) = tokio::io::duplex(128);
        let (prefs, local) = watch::channel(false);
        let (report, mut state) = watch::channel(false);
        let exchange = exchange(wire, Some(local), |active| {
            report.send_replace(active);
        });
        let peer_actions = async {
            assert_eq!(peer.read_u16().await?, 0);
            peer.write_all(&[PREFERENCE, 1, Kind::Text as u8, 0, 0, 0, 1, 0])
                .await?;

            // Malformed text sent while locally disabled must be discarded,
            // without accessing the Windows clipboard or breaking the stream.
            peer.write_all(&[PREFERENCE, 0, PREFERENCE, 1]).await?;
            tokio::task::yield_now().await;
            assert!(!*state.borrow());

            prefs.send_replace(true);
            assert_eq!(peer.read_u16().await?, 1);
            state.wait_for(|active| *active).await?;
            assert!(*state.borrow());

            prefs.send_replace(false);
            assert_eq!(peer.read_u16().await?, 0);
            assert!(!*state.borrow());
            Ok::<_, anyhow::Error>(())
        };
        timeout(Duration::from_secs(3), async {
            tokio::select! {
                result = exchange => bail!("Clipboard stream ended early: {result:?}"),
                result = peer_actions => result,
            }
        })
        .await?
    }

    #[test]
    fn rejects_paths_devices_and_unsafe_sizes() {
        for invalid in [
            "", ".", "..", "../test", "a\\b", "C:hi", "COM1.txt", "NUL", "LPTÂ²", "a.",
        ] {
            assert!(!safe_name(invalid), "{invalid}");
        }
        assert!(safe_name("foto 1.png"));
        assert!(parse_files(&[0]).is_err());
        assert!(parse_files(&[1, 0, 1, b'a', 0xff, 0xff, 0xff, 0xff]).is_err());
        assert!(validate_image(&[0; 40]).is_err());
    }

    #[test]
    fn validates_an_uncompressed_24_bit_bitmap() -> Result<()> {
        let mut bmp = vec![0; 56];
        bmp[..4].copy_from_slice(&40_u32.to_le_bytes());
        bmp[4..8].copy_from_slice(&2_i32.to_le_bytes());
        bmp[8..12].copy_from_slice(&2_i32.to_le_bytes());
        bmp[12..14].copy_from_slice(&1_u16.to_le_bytes());
        bmp[14..16].copy_from_slice(&24_u16.to_le_bytes());
        validate_image(&bmp)?;
        assert!(validate_image(&bmp[..55]).is_err());
        Ok(())
    }

    #[tokio::test]
    async fn stages_regular_files_without_using_remote_paths() -> Result<()> {
        let local = tempfile::tempdir()?;
        let file = local.path().join("example.txt");
        fs::write(&file, b"file contents").await?;
        let payload = encode_files(vec![file.to_string_lossy().into_owned()]).await?;
        let (destination, clipboard) = stage_files(&payload).await?;
        assert_eq!(clipboard.len(), 1);
        let stored = Path::new(&clipboard[0]);
        assert!(stored.starts_with(destination.path()));
        assert_eq!(fs::read(stored).await?, b"file contents");
        Ok(())
    }
}
