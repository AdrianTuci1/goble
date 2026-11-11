//! The end-to-end layer between a worker and a client.
//!
//! The relay forwards bytes it cannot read. Both ends run an X25519 exchange
//! signed by their certificates, derive one key per direction with HKDF-SHA256,
//! and wrap every frame in ChaCha20-Poly1305. A frame carries its counter in
//! the clear so the receiver can reject a replay; the counter is also the AEAD
//! nonce, so a rekey is required before it can wrap.

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use ring::agreement::{agree_ephemeral, EphemeralPrivateKey, UnparsedPublicKey, X25519};
use ring::hkdf::{Salt, HKDF_SHA256};
use ring::rand::SystemRandom;

/// Frames one direction may carry before the session must rekey.
pub const REKEY_AFTER_FRAMES: u64 = 1 << 20;

/// Bytes of a frame's clear-text counter.
pub const COUNTER_LEN: usize = 8;

/// The longest plaintext one frame may carry. Terminal output is chunked well
/// below this; the limit keeps a peer from making the relay buffer a stream.
pub const MAX_PLAINTEXT: usize = 1 << 20;

#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("the peer's public key was rejected")]
    InvalidPeerKey,
    #[error("the handshake failed: {0}")]
    Handshake(String),
    #[error("the frame is shorter than its counter")]
    ShortFrame,
    #[error("the frame is too large")]
    FrameTooLarge,
    #[error("the frame did not authenticate")]
    Decrypt,
    #[error("replayed frame: counter {got}, expected {expected}")]
    Replay { got: u64, expected: u64 },
    #[error("the session counter is exhausted; rekey first")]
    CounterExhausted,
}

/// One side's X25519 ephemeral key. Its public key goes in the signed
/// handshake; the private key never leaves the process and is consumed when the
/// shared secret is derived.
pub struct EphemeralSecret {
    private: EphemeralPrivateKey,
    public: [u8; 32],
}

impl EphemeralSecret {
    pub fn generate() -> Result<Self, CryptoError> {
        let rng = SystemRandom::new();
        let private = EphemeralPrivateKey::generate(&X25519, &rng)
            .map_err(|_| CryptoError::Handshake("no system randomness".into()))?;
        let public = private
            .compute_public_key()
            .map_err(|_| CryptoError::Handshake("could not compute the public key".into()))?;
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(public.as_ref());
        Ok(Self {
            private,
            public: bytes,
        })
    }

    pub fn public_key(&self) -> [u8; 32] {
        self.public
    }

    /// Derive the shared secret with the peer and turn it into the two
    /// directional frame ciphers. `transcript` is the hash of the signed
    /// handshake, so a key established in one session cannot be replayed in
    /// another.
    pub fn finish(self, peer_public: &[u8], transcript: &[u8]) -> Result<Session, CryptoError> {
        let peer = UnparsedPublicKey::new(&X25519, peer_public);
        let shared = agree_ephemeral(self.private, &peer, |material| {
            let mut bytes = [0u8; 32];
            bytes.copy_from_slice(material);
            bytes
        })
        .map_err(|_| CryptoError::InvalidPeerKey)?;
        Session::derive(shared, transcript)
    }
}

/// The key material and ciphers both ends derive from one handshake.
pub struct Session {
    /// The raw X25519 secret. Used only to derive the pairing SAS; it is never
    /// sent and never stored.
    pub shared: [u8; 32],
    /// Frames the handshake initiator sends.
    pub c2s: DirectionalCipher,
    /// Frames the responder sends.
    pub s2c: DirectionalCipher,
}

impl Session {
    fn derive(shared: [u8; 32], transcript: &[u8]) -> Result<Self, CryptoError> {
        let c2s = DirectionalCipher::derive(&shared, transcript, b"goble-e2ee-v1 c2s")?;
        let s2c = DirectionalCipher::derive(&shared, transcript, b"goble-e2ee-v1 s2c")?;
        Ok(Self { shared, c2s, s2c })
    }
}

/// One direction of a session: a counter, an expected counter and the AEAD it
/// keys.
pub struct DirectionalCipher {
    cipher: ChaCha20Poly1305,
    next_send: u64,
    next_receive: u64,
    sent: u64,
    received: u64,
}

impl DirectionalCipher {
    fn derive(shared: &[u8; 32], transcript: &[u8], info: &[u8]) -> Result<Self, CryptoError> {
        let salt = Salt::new(HKDF_SHA256, transcript);
        let prk = salt.extract(shared);
        let mut key = [0u8; 32];
        prk.expand(&[info], ring::hkdf::HKDF_SHA256)
            .map_err(|_| CryptoError::Handshake("HKDF rejected the info label".into()))?
            .fill(&mut key)
            .map_err(|_| CryptoError::Handshake("HKDF produced the wrong length".into()))?;
        let cipher = ChaCha20Poly1305::new_from_slice(&key)
            .map_err(|_| CryptoError::Handshake("invalid derived key".into()))?;
        // The derived key is not kept: `ChaCha20Poly1305` owns its schedule and
        // the raw bytes have no further use.
        Ok(Self {
            cipher,
            next_send: 1,
            next_receive: 1,
            sent: 0,
            received: 0,
        })
    }

    /// Wrap a plaintext. The first frame of a direction is counter 1; the
    /// counter is written big-endian so it reads the same in a capture.
    pub fn seal(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if plaintext.len() > MAX_PLAINTEXT {
            return Err(CryptoError::FrameTooLarge);
        }
        let counter = self.next_send;
        if counter == u64::MAX {
            return Err(CryptoError::CounterExhausted);
        }
        let nonce = nonce_for(counter);
        let ciphertext = self
            .cipher
            .encrypt(Nonce::from_slice(&nonce), plaintext)
            .map_err(|_| CryptoError::Decrypt)?;
        let mut frame = Vec::with_capacity(COUNTER_LEN + ciphertext.len());
        frame.extend_from_slice(&counter.to_be_bytes());
        frame.extend_from_slice(&ciphertext);
        self.next_send += 1;
        self.sent += 1;
        Ok(frame)
    }

    /// Open a frame, requiring the exact next counter. Frames are carried by an
    /// ordered stream (WebSocket over TCP), so a gap is an error, not something
    /// to buffer.
    pub fn open(&mut self, frame: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if frame.len() < COUNTER_LEN {
            return Err(CryptoError::ShortFrame);
        }
        // The authentication tag is 16 bytes; anything larger than the
        // plaintext ceiling plus that tag cannot be a valid frame.
        if frame.len() > COUNTER_LEN + MAX_PLAINTEXT + 16 {
            return Err(CryptoError::FrameTooLarge);
        }
        let counter = u64::from_be_bytes(
            frame[..COUNTER_LEN]
                .try_into()
                .map_err(|_| CryptoError::ShortFrame)?,
        );
        if counter != self.next_receive {
            return Err(CryptoError::Replay {
                got: counter,
                expected: self.next_receive,
            });
        }
        let nonce = nonce_for(counter);
        let plaintext = self
            .cipher
            .decrypt(Nonce::from_slice(&nonce), &frame[COUNTER_LEN..])
            .map_err(|_| CryptoError::Decrypt)?;
        self.next_receive += 1;
        self.received += 1;
        Ok(plaintext)
    }

    /// Whether this direction should rekey before its next frame.
    pub fn needs_rekey(&self) -> bool {
        self.sent >= REKEY_AFTER_FRAMES || self.received >= REKEY_AFTER_FRAMES
    }

    /// Frames this direction has sealed.
    pub fn sent(&self) -> u64 {
        self.sent
    }

    /// Frames this direction has opened.
    pub fn received(&self) -> u64 {
        self.received
    }
}

fn nonce_for(counter: u64) -> [u8; 12] {
    let mut nonce = [0u8; 12];
    nonce[4..].copy_from_slice(&counter.to_be_bytes());
    nonce
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::RngCore;

    fn transcript() -> [u8; 32] {
        let mut t = [0u8; 32];
        rand::rng().fill_bytes(&mut t);
        t
    }

    fn pair() -> (Session, Session) {
        let a = EphemeralSecret::generate().expect("a key");
        let b = EphemeralSecret::generate().expect("b key");
        let a_pub = a.public_key();
        let b_pub = b.public_key();
        let transcript = transcript();
        let a_session = a.finish(&b_pub, &transcript).expect("a session");
        let b_session = b.finish(&a_pub, &transcript).expect("b session");
        (a_session, b_session)
    }

    #[test]
    fn both_ends_derive_the_same_secret() {
        let (a, b) = pair();
        assert_eq!(a.shared, b.shared);
    }

    #[test]
    fn a_different_transcript_derives_a_different_key() {
        let a = EphemeralSecret::generate().unwrap();
        let b = EphemeralSecret::generate().unwrap();
        let a_pub = a.public_key();
        let b_pub = b.public_key();
        let mut first = a.finish(&b_pub, b"first").unwrap();
        let mut second = b.finish(&a_pub, b"second").unwrap();
        assert_eq!(
            first.shared, second.shared,
            "X25519 itself is transcript-free"
        );
        assert_ne!(
            first.c2s.seal(b"x").unwrap(),
            second.c2s.seal(b"x").unwrap(),
            "the derived frame keys differ with the transcript"
        );
    }

    #[test]
    fn frames_round_trip_in_both_directions() {
        let (mut a, mut b) = pair();
        let from_a = a.c2s.seal(b"hello").unwrap();
        assert_eq!(b.c2s.open(&from_a).unwrap(), b"hello");
        let from_b = b.s2c.seal(b"world").unwrap();
        assert_eq!(a.s2c.open(&from_b).unwrap(), b"world");
        assert_eq!(a.c2s.sent(), 1);
        assert_eq!(b.c2s.received(), 1);
    }

    #[test]
    fn a_tampered_frame_does_not_authenticate() {
        let (mut a, mut b) = pair();
        let mut frame = a.c2s.seal(b"payload").unwrap();
        let last = frame.len() - 1;
        frame[last] ^= 0x01;
        assert!(matches!(b.c2s.open(&frame), Err(CryptoError::Decrypt)));
    }

    #[test]
    fn a_replayed_frame_is_rejected() {
        let (mut a, mut b) = pair();
        let frame = a.c2s.seal(b"one").unwrap();
        assert_eq!(b.c2s.open(&frame).unwrap(), b"one");
        assert!(matches!(
            b.c2s.open(&frame),
            Err(CryptoError::Replay {
                got: 1,
                expected: 2
            })
        ));
    }

    #[test]
    fn an_out_of_order_frame_is_rejected() {
        let (mut a, mut b) = pair();
        let _first = a.c2s.seal(b"one").unwrap();
        let second = a.c2s.seal(b"two").unwrap();
        assert!(matches!(
            b.c2s.open(&second),
            Err(CryptoError::Replay {
                got: 2,
                expected: 1
            })
        ));
    }

    #[test]
    fn an_unknown_peer_key_is_refused() {
        let a = EphemeralSecret::generate().unwrap();
        assert!(matches!(
            a.finish(&[0u8; 32], b"t"),
            Err(CryptoError::InvalidPeerKey)
        ));
    }

    #[test]
    fn a_frame_over_the_limit_is_refused() {
        let (mut a, _b) = pair();
        let oversized = vec![0u8; MAX_PLAINTEXT + 1];
        assert!(matches!(
            a.c2s.seal(&oversized),
            Err(CryptoError::FrameTooLarge)
        ));
    }

    #[test]
    fn a_short_frame_is_refused() {
        let (mut _a, mut b) = pair();
        assert!(matches!(
            b.c2s.open(&[0u8; 3]),
            Err(CryptoError::ShortFrame)
        ));
    }

    #[test]
    fn an_oversized_frame_is_refused_before_decryption() {
        let (_a, mut b) = pair();
        let frame = vec![0u8; COUNTER_LEN + MAX_PLAINTEXT + 17];
        assert!(matches!(
            b.c2s.open(&frame),
            Err(CryptoError::FrameTooLarge)
        ));
    }

    #[test]
    fn rekey_is_signalled_after_the_frame_budget() {
        let (mut a, _b) = pair();
        assert!(!a.c2s.needs_rekey());
        for _ in 0..REKEY_AFTER_FRAMES {
            a.c2s.seal(b"x").unwrap();
        }
        assert!(a.c2s.needs_rekey());
    }
}
