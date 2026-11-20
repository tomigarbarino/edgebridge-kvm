use std::sync::Arc;

use anyhow::{Result, anyhow, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rcgen::CertifiedKey;
use rustls::{
    ClientConfig, DigitallySignedStruct, Error, ServerConfig, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{WebPkiSupportedAlgorithms, ring, verify_tls12_signature, verify_tls13_signature},
    pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName, UnixTime},
};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::protocol::Token;

const PREFIX: &str = "EB1-";
const ALPN: &[u8] = b"edgebridge-kvm/1";
pub const APPROVAL_ALPN: &[u8] = b"edgebridge-kvm/3-approval";
pub const APPROVAL_EXPORTER: &[u8] = b"EXPORTER-EdgeBridge-pairing-confirmation";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Invite {
    pin: [u8; 32],
    pub token: Token,
}

impl Invite {
    fn encode(self) -> String {
        let mut bytes = [0; 64];
        bytes[..32].copy_from_slice(&self.pin);
        bytes[32..].copy_from_slice(&self.token);
        format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes))
    }

    pub fn parse(text: &str) -> Result<Self> {
        let payload = text
            .trim()
            .strip_prefix(PREFIX)
            .ok_or_else(|| anyhow!("Invalid pairing code"))?;
        ensure!(payload.len() == 86, "Invalid pairing code length");
        let mut bytes = [0; 64];
        ensure!(
            URL_SAFE_NO_PAD.decode_slice(payload, &mut bytes)? == bytes.len(),
            "Invalid pairing code"
        );
        Ok(Self {
            pin: bytes[..32].try_into()?,
            token: bytes[32..].try_into()?,
        })
    }

    pub fn client(self) -> Result<ClientConfig> {
        client(self.pin)
    }

    pub fn pin(self) -> [u8; 32] {
        self.pin
    }

    pub fn from_approval(pin: [u8; 32], token: Token) -> Self {
        Self { pin, token }
    }
}

pub fn client(pin: [u8; 32]) -> Result<ClientConfig> {
    let provider = ring::default_provider();
    let verifier = PinVerifier {
        pin,
        algorithms: provider.signature_verification_algorithms,
    };
    let mut config = ClientConfig::builder_with_provider(Arc::new(provider))
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    config.alpn_protocols = vec![ALPN.to_vec()];
    Ok(config)
}

pub fn confirmation(bytes: [u8; 8]) -> String {
    format!("{:08}", u64::from_be_bytes(bytes) % 100_000_000)
}

pub fn create() -> Result<(ServerConfig, String, Token)> {
    let CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["edgebridge.local".into()])?;
    let mut token = [0; 32];
    getrandom::fill(&mut token).map_err(|e| anyhow!("Could not generate pairing secret: {e}"))?;
    let invite = Invite {
        pin: Sha256::digest(cert.der().as_ref()).into(),
        token,
    };
    let mut config = ServerConfig::builder_with_provider(Arc::new(ring::default_provider()))
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der().clone()],
            PrivatePkcs8KeyDer::from(signing_key.serialize_der()).into(),
        )?;
    config.alpn_protocols = vec![ALPN.to_vec()];
    Ok((config, invite.encode(), token))
}

pub fn authenticate(actual: Token, expected: Token) -> bool {
    bool::from(actual.ct_eq(&expected))
}

#[derive(Debug)]
struct PinVerifier {
    pin: [u8; 32],
    algorithms: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for PinVerifier {
    fn verify_server_cert(
        &self,
        cert: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        let actual: [u8; 32] = Sha256::digest(cert.as_ref()).into();
        if bool::from(actual.ct_eq(&self.pin)) {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls12_signature(message, cert, signature, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        signature: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature(message, cert, signature, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_an_invite_and_rejects_bad_codes() -> Result<()> {
        let (_, code, token) = create()?;
        assert!(authenticate(Invite::parse(&code)?.token, token));
        assert!(!authenticate([0; 32], token));
        assert!(Invite::parse("EB1-test").is_err());
        Ok(())
    }
}
