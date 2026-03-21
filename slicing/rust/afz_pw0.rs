//! PW0 run-length encoding for Anycubic layer images.
//!
//! PW0 uses 4-bit colour quantisation (16 grey levels).
//!
//! Encoding rules:
//! - Each pixel is quantised to `colour = byte >> 4` (0x0..0xF).
//! - **Black (0x0) or white (0xF)**: 2-byte big-endian encoding
//!   `[colour_nibble << 12 | repeat]` with max repeat 4095.
//! - **Grey (0x1..0xE)**: 1-byte encoding `[colour_nibble << 4 | repeat]`
//!   with max repeat 15.

const RLE4_ENCODING_LIMIT: u16 = 0xFFF; // 4095

/// Encode a raw 8-bit greyscale mask into PW0 RLE bytes.
pub(super) fn encode_pw0(mask: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(mask.len() / 4);
    let mut last_colour: i32 = -1;
    let mut reps: u32 = 0;

    #[inline]
    fn flush(out: &mut Vec<u8>, colour: i32, reps: &mut u32) {
        while *reps > 0 {
            if colour == 0x0 || colour == 0xF {
                let done = (*reps).min(RLE4_ENCODING_LIMIT as u32);
                let word = (done as u16) | ((colour as u16) << 12);
                out.push((word >> 8) as u8);
                out.push(word as u8);
                *reps -= done;
            } else {
                let done = (*reps).min(0xF);
                out.push((done as u8) | ((colour as u8) << 4));
                *reps -= done;
            }
        }
    }

    for &byte in mask {
        let colour = (byte >> 4) as i32;
        if colour == last_colour {
            reps += 1;
        } else {
            flush(&mut out, last_colour, &mut reps);
            last_colour = colour;
            reps = 1;
        }
    }
    flush(&mut out, last_colour, &mut reps);

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_all_black() {
        let mask = vec![0u8; 100];
        let encoded = encode_pw0(&mask);
        // Black run of 100: 0x0064
        assert_eq!(encoded, vec![0x00, 100]);
    }

    #[test]
    fn encode_all_white() {
        let mask = vec![255u8; 100];
        let encoded = encode_pw0(&mask);
        // White run of 100: 0xF064
        assert_eq!(encoded, vec![0xF0, 100]);
    }

    #[test]
    fn encode_grey_limited_to_15_reps() {
        // Grey colour 0x8 (pixel value 0x80..0x8F) with 20 reps
        let mask = vec![0x88u8; 20];
        let encoded = encode_pw0(&mask);
        // 15 reps + 5 reps
        assert_eq!(encoded, vec![0x8F, 0x85]);
    }

    #[test]
    fn encode_mixed() {
        // 3 black, 2 white
        let mask = vec![0, 0, 0, 255, 255];
        let encoded = encode_pw0(&mask);
        assert_eq!(encoded, vec![0x00, 3, 0xF0, 2]);
    }

    #[test]
    fn encode_long_black_run_above_4095() {
        let mask = vec![0u8; 5000];
        let encoded = encode_pw0(&mask);
        // 4095 + 905 = 0x0FFF then 0x0389
        assert_eq!(
            encoded,
            vec![0x0F, 0xFF, 0x03, 0x89]
        );
    }
}
