//! AFF (Anycubic File Format) binary container assembly.
//!
//! Writes a sequential binary file:
//!   FileMark (12 + N bytes) -> Header table -> Preview table ->
//!   [optional version-gated tables] -> LayerDef table (with backpatched
//!   per-layer DataAddress/DataLength) -> raw layer RLE blob.
//!
//! Each "named table" begins with a 12-byte NUL-padded ASCII name + 4-byte LE
//! body length. Address fields in FileMark are backpatched after table writes.

use std::io::{Cursor, Seek, SeekFrom, Write};

// ── Primitive writers ───────────────────────────────────────────────

pub(super) fn write_u32_le(out: &mut Cursor<Vec<u8>>, v: u32) -> std::io::Result<()> {
    out.write_all(&v.to_le_bytes())
}

pub(super) fn write_f32_le(out: &mut Cursor<Vec<u8>>, v: f32) -> std::io::Result<()> {
    out.write_all(&v.to_le_bytes())
}

/// Write a string padded with NUL bytes to exactly `fixed_len` bytes.
/// Truncates if `value` is longer (leaving room for at least one NUL terminator).
pub(super) fn write_padded_string(
    out: &mut Cursor<Vec<u8>>,
    value: &str,
    fixed_len: usize,
) -> std::io::Result<()> {
    let bytes = value.as_bytes();
    let max_copy = if bytes.len() >= fixed_len {
        fixed_len.saturating_sub(1)
    } else {
        bytes.len()
    };
    let mut buf = vec![0u8; fixed_len];
    buf[..max_copy].copy_from_slice(&bytes[..max_copy]);
    out.write_all(&buf)
}

/// Write the 12-byte NUL-padded section name + 4-byte LE body length.
pub(super) fn write_named_table_header(
    out: &mut Cursor<Vec<u8>>,
    name: &str,
    body_length: u32,
) -> std::io::Result<()> {
    write_padded_string(out, name, 12)?;
    write_u32_le(out, body_length)
}

/// Overwrite a u32 at a known absolute offset in the buffer.
pub(super) fn patch_u32_le(out: &mut Cursor<Vec<u8>>, abs_offset: u64, v: u32) -> std::io::Result<()> {
    let saved = out.position();
    out.seek(SeekFrom::Start(abs_offset))?;
    out.write_all(&v.to_le_bytes())?;
    out.seek(SeekFrom::Start(saved))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u32_le_writes_four_bytes() {
        let mut c = Cursor::new(Vec::new());
        write_u32_le(&mut c, 0x01020304).unwrap();
        assert_eq!(c.into_inner(), vec![0x04, 0x03, 0x02, 0x01]);
    }

    #[test]
    fn padded_string_pads_with_nulls() {
        let mut c = Cursor::new(Vec::new());
        write_padded_string(&mut c, "HI", 6).unwrap();
        assert_eq!(c.into_inner(), vec![b'H', b'I', 0, 0, 0, 0]);
    }

    #[test]
    fn padded_string_truncates_long_input_leaving_null_terminator() {
        let mut c = Cursor::new(Vec::new());
        write_padded_string(&mut c, "ABCDEFGHIJ", 6).unwrap();
        // Truncated to 5 chars + 1 null
        let result = c.into_inner();
        assert_eq!(result.len(), 6);
        assert_eq!(&result[..5], b"ABCDE");
        assert_eq!(result[5], 0);
    }

    #[test]
    fn named_table_header_writes_12_byte_name_plus_4_byte_length() {
        let mut c = Cursor::new(Vec::new());
        write_named_table_header(&mut c, "HEADER", 80).unwrap();
        let bytes = c.into_inner();
        assert_eq!(bytes.len(), 16);
        assert_eq!(&bytes[..6], b"HEADER");
        assert_eq!(&bytes[6..12], &[0, 0, 0, 0, 0, 0]);
        assert_eq!(&bytes[12..16], &80u32.to_le_bytes());
    }

    #[test]
    fn patch_overwrites_in_place_and_restores_position() {
        let mut c = Cursor::new(vec![0u8; 8]);
        c.seek(SeekFrom::Start(4)).unwrap();
        patch_u32_le(&mut c, 0, 0xDEADBEEF).unwrap();
        assert_eq!(c.position(), 4, "position not restored");
        let bytes = c.into_inner();
        assert_eq!(&bytes[..4], &0xDEADBEEFu32.to_le_bytes());
    }
}
