use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
    ptr::{null, null_mut},
};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    },
};

use crate::{discovery::valid_name, protocol::Token};

#[derive(Deserialize, Serialize)]
pub struct Peer {
    pub name: String,
    pub secret: Token,
}

#[derive(Default)]
pub struct Store(Option<PathBuf>);

impl Store {
    pub fn new(path: PathBuf) -> Self {
        Self(Some(path))
    }

    pub fn load(&self) -> Result<Option<Peer>> {
        let Some(path) = &self.0 else { return Ok(None) };
        let mut bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let peer: Peer = serde_json::from_slice(&protect(&mut bytes, false)?)?;
        ensure!(valid_name(&peer.name), "Invalid remembered PC");
        Ok(Some(peer))
    }

    pub fn save(&self, peer: Peer) -> Result<()> {
        let path = self.0.as_ref().context("Device storage is unavailable")?;
        let parent = path.parent().context("Device storage is unavailable")?;
        let bytes = protect(&mut serde_json::to_vec(&peer)?, true)?;
        fs::create_dir_all(parent)?;
        let mut staged = tempfile::NamedTempFile::new_in(parent)?;
        staged.write_all(&bytes)?;
        staged.as_file().sync_all()?;
        staged.persist(path)?;
        Ok(())
    }

    pub fn forget(&self) -> Result<()> {
        if let Some(path) = &self.0
            && let Err(error) = fs::remove_file(path)
            && error.kind() != io::ErrorKind::NotFound
        {
            return Err(error.into());
        }
        Ok(())
    }
}

fn protect(bytes: &mut [u8], encrypt: bool) -> Result<Vec<u8>> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len().try_into()?,
        pbData: bytes.as_mut_ptr(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let flags = CRYPTPROTECT_UI_FORBIDDEN;
    unsafe {
        let success = if encrypt {
            CryptProtectData(&input, null(), null(), null(), null(), flags, &mut output)
        } else {
            CryptUnprotectData(
                &input,
                null_mut(),
                null(),
                null(),
                null(),
                flags,
                &mut output,
            )
        };
        if success == 0 {
            return Err(io::Error::last_os_error().into());
        }
        ensure!(
            !output.pbData.is_null(),
            "Windows returned an empty credential"
        );
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        LocalFree(output.pbData.cast());
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembered_peer_survives_reopening_and_can_be_revoked() -> Result<()> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("device.bin");
        Store::new(path.clone()).save(Peer {
            name: "Secondary".into(),
            secret: [42; 32],
        })?;
        let store = Store::new(path);
        assert_eq!(store.load()?.context("Missing device")?.secret, [42; 32]);
        store.forget()?;
        assert!(store.load()?.is_none());
        Ok(())
    }
}
