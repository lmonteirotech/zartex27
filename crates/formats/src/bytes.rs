//! Little-endian reads at a byte offset, shared by the format readers.
//! ORIGIN NOTICE: this file was brought into this repository from the in-development "game engine Rust Rework research"
//! project. It was not written for this repository. Differences from that version:
//! - not itemised for this file
//!
//! It exposes `u16_le`, `u32_le` and `f32_le`. Each returns `None` when the slice is too short; the readers keep their own
//! error style on top (a `Result` with their `Error`, an `Option`, or 0 when missing).
//! `chnk` and `vehicle` use these functions.

/// Reads the little-endian `u16` at byte offset `o`. Returns `None` when fewer than 2 bytes are available there.
pub fn u16_le(b: &[u8], o: usize) -> Option<u16> {
    b.get(o..o + 2).map(|s| u16::from_le_bytes([s[0], s[1]]))
}

/// Reads the little-endian `u32` at byte offset `o`. Returns `None` when fewer than 4 bytes are available there.
pub fn u32_le(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// Reads the `f32` at byte offset `o`: the `u32` at `o` reinterpreted as its bits. Returns `None` when fewer than 4 bytes
/// are available there.
pub fn f32_le(b: &[u8], o: usize) -> Option<f32> {
    u32_le(b, o).map(f32::from_bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_bounds() {
        let b = [0x01, 0x02, 0x03, 0x04, 0x00, 0x00, 0x80, 0x3F];
        assert_eq!(u16_le(&b, 0), Some(0x0201));
        assert_eq!(u32_le(&b, 0), Some(0x0403_0201));
        assert_eq!(f32_le(&b, 4), Some(1.0));
        assert_eq!(u32_le(&b, 5), None);
        assert_eq!(u16_le(&b, 7), None);
    }
}
