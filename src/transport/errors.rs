#[derive(thiserror::Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportError {
    #[error("Not connected")]
    NotConnected,
    #[error("Connection refused")]
    ConnectionRefused,
    #[error("Connection timed out")]
    Timeout,
    #[error("Transport disconnected unexpectedly")]
    BrokenPipe,
    #[error("Payload too large for transport")]
    PayloadTooLarge,
    #[error("Encryption error occurred")]
    EncryptionError,
}
