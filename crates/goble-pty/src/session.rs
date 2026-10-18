//! The per-session replay ring.
//!
//! Output is kept byte-exact in a bounded FIFO. `start_seq` is the absolute
//! offset of the oldest retained byte, so a client's `from_seq` always means
//! the same thing: the offset of the next byte it wants, not a message index.
//! A ring smaller than `REPLAY_CAPACITY` would silently lose the prefix a
//! re-attaching client asks for, so the capacity is fixed at 512 KiB.

use std::collections::VecDeque;

/// The replay window every session keeps.
pub const REPLAY_CAPACITY: usize = 512 * 1024;

const _: () = assert!(REPLAY_CAPACITY >= 256 * 1024);

/// How much output one `output` event carries during replay.
pub const OUTPUT_CHUNK: usize = 8 * 1024;

/// A bounded, byte-exact history of one session's output.
#[derive(Debug, Clone)]
pub struct ReplayRing {
    data: VecDeque<u8>,
    start_seq: u64,
    capacity: usize,
}

impl Default for ReplayRing {
    fn default() -> Self {
        Self::new(REPLAY_CAPACITY)
    }
}

impl ReplayRing {
    /// A ring holding at most `capacity` bytes. Zero is treated as one byte so
    /// a ring always makes progress.
    pub fn new(capacity: usize) -> Self {
        Self {
            data: VecDeque::new(),
            start_seq: 0,
            capacity: capacity.max(1),
        }
    }

    /// Append `bytes`; returns the sequence (byte offset) of the first one.
    pub fn push(&mut self, bytes: &[u8]) -> u64 {
        let seq = self.end_seq();
        self.data.extend(bytes.iter().copied());
        while self.data.len() > self.capacity {
            self.data.pop_front();
        }
        // Bytes dropped from the front advance the offset by the same amount.
        let kept = self.data.len();
        let total = seq + bytes.len() as u64;
        self.start_seq = total.saturating_sub(kept as u64);
        seq
    }

    /// Offset of the oldest retained byte.
    pub fn start_seq(&self) -> u64 {
        self.start_seq
    }

    /// Offset one past the newest retained byte.
    pub fn end_seq(&self) -> u64 {
        self.start_seq + self.data.len() as u64
    }

    /// How many bytes are retained.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// The retained bytes from `from_seq` onward, and the offset they start at.
    ///
    /// A `from_seq` older than the window is answered from the oldest retained
    /// byte (the window is the guarantee, not an infinite log).
    pub fn read_from(&self, from_seq: u64) -> (u64, Vec<u8>) {
        let start = from_seq.max(self.start_seq).min(self.end_seq());
        let skip = (start - self.start_seq) as usize;
        (start, self.data.iter().skip(skip).copied().collect())
    }

    /// Split retained bytes at `seq` into replay-sized chunks.
    pub fn chunks_from(&self, from_seq: u64) -> Vec<(u64, Vec<u8>)> {
        let (start, bytes) = self.read_from(from_seq);
        bytes
            .chunks(OUTPUT_CHUNK)
            .enumerate()
            .map(|(index, chunk)| {
                let seq = start + (index * OUTPUT_CHUNK) as u64;
                (seq, chunk.to_vec())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_are_byte_offsets_and_replay_is_exact() {
        let mut ring = ReplayRing::new(REPLAY_CAPACITY);
        assert_eq!(ring.push(b"hello"), 0);
        assert_eq!(ring.push(b" world"), 5);
        assert_eq!(ring.start_seq(), 0);
        assert_eq!(ring.end_seq(), 11);
        assert_eq!(ring.read_from(0), (0, b"hello world".to_vec()));
        assert_eq!(ring.read_from(6), (6, b"world".to_vec()));
        assert_eq!(ring.read_from(11), (11, Vec::new()));
        assert_eq!(ring.read_from(99), (11, Vec::new()));
    }

    #[test]
    fn the_ring_stays_at_its_cap_and_keeps_a_byte_exact_suffix() {
        let mut ring = ReplayRing::new(8);
        let mut all = Vec::new();
        for i in 0..20u8 {
            all.push(i);
            ring.push(&[i]);
        }
        assert_eq!(ring.len(), 8);
        assert_eq!(ring.start_seq(), 12);
        assert_eq!(ring.end_seq(), 20);
        let (start, bytes) = ring.read_from(0);
        assert_eq!(start, 12);
        assert_eq!(bytes, all[12..].to_vec());
        let (start, bytes) = ring.read_from(17);
        assert_eq!(start, 17);
        assert_eq!(bytes, all[17..].to_vec());
    }

    #[test]
    fn the_default_window_holds_at_least_256_kib_byte_exactly() {
        let mut ring = ReplayRing::default();
        let payload: Vec<u8> = (0..REPLAY_CAPACITY).map(|i| (i % 251) as u8).collect();
        ring.push(&payload);
        assert_eq!(ring.len(), REPLAY_CAPACITY);
        assert_eq!(ring.start_seq(), 0);
        assert_eq!(ring.read_from(0).1, payload);
    }

    #[test]
    fn replay_is_chunked_without_changing_bytes() {
        let mut ring = ReplayRing::new(REPLAY_CAPACITY);
        let payload: Vec<u8> = (0..(OUTPUT_CHUNK * 2 + 17)).map(|i| i as u8).collect();
        ring.push(&payload);
        let chunks = ring.chunks_from(0);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].0, 0);
        assert_eq!(chunks[1].0, OUTPUT_CHUNK as u64);
        assert_eq!(chunks[2].0, (OUTPUT_CHUNK * 2) as u64);
        let joined: Vec<u8> = chunks.into_iter().flat_map(|(_, bytes)| bytes).collect();
        assert_eq!(joined, payload);
    }
}
