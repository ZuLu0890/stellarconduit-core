use crate::message::types::TransactionEnvelope;
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};

#[derive(Debug, thiserror::Error)]
pub enum EnvelopeError {
    #[error("message_id is all zeros")]
    ZeroMessageId,
    #[error("tx_xdr is empty")]
    EmptyXdr,
    #[error("ttl_hops is zero — envelope is already expired")]
    ZeroTtl,
    #[error("timestamp is zero")]
    ZeroTimestamp,
    #[error("message_id does not match canonical hash")]
    MessageIdMismatch,
}

/// SHA-256( origin_pubkey ‖ tx_xdr_bytes ‖ timestamp_le64 )
pub fn compute_message_id(origin_pubkey: &[u8; 32], tx_xdr: &str, timestamp: u64) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(origin_pubkey);
    h.update(tx_xdr.as_bytes());
    h.update(timestamp.to_le_bytes());
    h.finalize().into()
}

pub fn validate_envelope(env: &TransactionEnvelope) -> Result<(), EnvelopeError> {
    if env.message_id == [0u8; 32] {
        return Err(EnvelopeError::ZeroMessageId);
    }
    if env.tx_xdr.is_empty() {
        return Err(EnvelopeError::EmptyXdr);
    }
    if env.ttl_hops == 0 {
        return Err(EnvelopeError::ZeroTtl);
    }
    if env.timestamp == 0 {
        return Err(EnvelopeError::ZeroTimestamp);
    }
    if env.message_id != compute_message_id(&env.origin_pubkey, &env.tx_xdr, env.timestamp) {
        return Err(EnvelopeError::MessageIdMismatch);
    }
    Ok(())
}

pub struct EnvelopeBuilder {
    origin_pubkey: [u8; 32],
    tx_xdr: String,
    ttl_hops: u8,
    timestamp: Option<u64>,
}

impl EnvelopeBuilder {
    pub fn new(origin_pubkey: [u8; 32], tx_xdr: impl Into<String>) -> Self {
        Self {
            origin_pubkey,
            tx_xdr: tx_xdr.into(),
            ttl_hops: 10,
            timestamp: None,
        }
    }

    pub fn ttl(mut self, hops: u8) -> Self {
        self.ttl_hops = hops;
        self
    }

    pub fn timestamp(mut self, ts: u64) -> Self {
        self.timestamp = Some(ts);
        self
    }

    pub fn build(self, signing_key: &SigningKey) -> TransactionEnvelope {
        let timestamp = self.timestamp.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        });
        let message_id = compute_message_id(&self.origin_pubkey, &self.tx_xdr, timestamp);
        let signature = signing_key.sign(&message_id).to_bytes();
        TransactionEnvelope {
            message_id,
            origin_pubkey: self.origin_pubkey,
            tx_xdr: self.tx_xdr,
            ttl_hops: self.ttl_hops,
            timestamp,
            signature,
        }
    }

    #[cfg(test)]
    pub fn build_unsigned(self) -> TransactionEnvelope {
        let timestamp = self.timestamp.unwrap_or(1);
        let message_id = compute_message_id(&self.origin_pubkey, &self.tx_xdr, timestamp);
        TransactionEnvelope {
            message_id,
            origin_pubkey: self.origin_pubkey,
            tx_xdr: self.tx_xdr,
            ttl_hops: self.ttl_hops,
            timestamp,
            signature: [0u8; 64],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    fn signing_key() -> SigningKey {
        SigningKey::generate(&mut OsRng)
    }

    #[test]
    fn test_message_id_is_deterministic() {
        let pk = [1u8; 32];
        assert_eq!(
            compute_message_id(&pk, "xdr", 999),
            compute_message_id(&pk, "xdr", 999)
        );
    }

    #[test]
    fn test_message_id_changes_with_xdr() {
        let pk = [1u8; 32];
        assert_ne!(
            compute_message_id(&pk, "xdr_a", 999),
            compute_message_id(&pk, "xdr_b", 999)
        );
    }

    #[test]
    fn test_builder_produces_valid_envelope() {
        let key = signing_key();
        let env = EnvelopeBuilder::new(key.verifying_key().to_bytes(), "some_xdr")
            .timestamp(1_000_000)
            .build(&key);
        assert!(validate_envelope(&env).is_ok());
    }

    #[test]
    fn test_validate_rejects_zero_message_id() {
        let key = signing_key();
        let mut env = EnvelopeBuilder::new(key.verifying_key().to_bytes(), "some_xdr")
            .timestamp(1_000_000)
            .build(&key);
        env.message_id = [0u8; 32];
        assert!(matches!(
            validate_envelope(&env),
            Err(EnvelopeError::ZeroMessageId)
        ));
    }

    #[test]
    fn test_validate_rejects_message_id_mismatch() {
        let key = signing_key();
        let mut env = EnvelopeBuilder::new(key.verifying_key().to_bytes(), "some_xdr")
            .timestamp(1_000_000)
            .build(&key);
        env.tx_xdr = "tampered".to_string();
        assert!(matches!(
            validate_envelope(&env),
            Err(EnvelopeError::MessageIdMismatch)
        ));
    }
}
