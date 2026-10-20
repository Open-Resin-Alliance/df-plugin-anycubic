//! AFF (Anycubic File Format) binary container assembly.
//!
//! Writes a sequential binary file:
//!   FileMark (12 + N bytes) -> Header table -> Preview table ->
//!   [optional version-gated tables] -> LayerDef table -> raw layer RLE blob.
//!
//! Each "named table" begins with a 12-byte NUL-padded ASCII name + 4-byte LE
//! body length. Absolute addresses are calculated before the sequential writes.

use std::fs::File;
use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};

use super::layer_spool::StoredLayer;

// ── Primitive writers ───────────────────────────────────────────────

pub(super) fn write_u32_le<W: Write>(out: &mut W, v: u32) -> std::io::Result<()> {
    out.write_all(&v.to_le_bytes())
}

pub(super) fn write_f32_le<W: Write>(out: &mut W, v: f32) -> std::io::Result<()> {
    out.write_all(&v.to_le_bytes())
}

/// Write a string padded with NUL bytes to exactly `fixed_len` bytes.
/// Truncates if `value` is longer (leaving room for at least one NUL terminator).
pub(super) fn write_padded_string<W: Write>(
    out: &mut W,
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
pub(super) fn write_named_table_header<W: Write>(
    out: &mut W,
    name: &str,
    body_length: u32,
) -> std::io::Result<()> {
    write_padded_string(out, name, 12)?;
    write_u32_le(out, body_length)
}

/// Overwrite a u32 at a known absolute offset in the buffer.
#[allow(dead_code)]
pub(super) fn patch_u32_le<W: Write + Seek>(out: &mut W, abs_offset: u64, v: u32) -> std::io::Result<()> {
    let saved = out.stream_position()?;
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

// ═══════════════════════════════════════════════════════════════════
//  Table writers (per-table body bytes; caller emits named header)
// ═══════════════════════════════════════════════════════════════════

use super::aff_metadata::{AffBuildModel, AffMachineProfile, AffTimingModel};

// Header body lengths from the C# SerializeWhen attribute table
pub(super) const HEADER_LEN_V1: u32 = 80;
pub(super) const HEADER_LEN_V515: u32 = 80;
pub(super) const HEADER_LEN_V516: u32 = 84;
pub(super) const HEADER_LEN_V517: u32 = 92;
pub(super) const HEADER_LEN_V518: u32 = 96;

pub(super) fn header_body_length_for_version(v: u16) -> u32 {
    match v {
        518 => HEADER_LEN_V518,
        517 => HEADER_LEN_V517,
        516 => HEADER_LEN_V516,
        515 => HEADER_LEN_V515,
        _ => HEADER_LEN_V1,
    }
}

pub(super) fn write_header_body<W: Write + Seek>(
    out: &mut W,
    version: u16,
    resolution_x: u32,
    resolution_y: u32,
    timing: &AffTimingModel,
    build: &AffBuildModel,
    profile: &AffMachineProfile,
    print_time_sec: u32,
    volume_ml: f32,
    weight_g: f32,
    price: f32,
    per_layer_settings: bool,
) -> std::io::Result<()> {
    let _ = build; // build dims go into Machine table, not Header
    let start_pos = out.stream_position()?;

    let total_lift = timing.lift_height_mm + timing.lift_height2_mm;
    let advanced_mode_flag: u32 = if version >= 516 && (timing.lift_height2_mm > 0.0 || timing.bottom_lift_height2_mm > 0.0) { 1 } else { 0 };

    write_f32_le(out, profile.default_pixel_um)?;
    write_f32_le(out, timing.layer_height_mm)?;
    write_f32_le(out, timing.normal_exposure_sec)?;
    write_f32_le(out, timing.wait_time_before_cure_sec)?;
    write_f32_le(out, timing.bottom_exposure_sec)?;
    write_f32_le(out, timing.bottom_layer_count as f32)?;
    write_f32_le(out, if version >= 516 { total_lift } else { timing.lift_height_mm })?;
    write_f32_le(out, timing.lift_speed_mm_s)?;
    write_f32_le(out, timing.retract_speed_mm_s)?;
    write_f32_le(out, volume_ml)?;
    write_u32_le(out, timing.anti_alias_level.clamp(1, 16))?; // AntiAliasing (grey level count)
    write_u32_le(out, resolution_x)?;
    write_u32_le(out, resolution_y)?;
    write_f32_le(out, weight_g)?;
    write_f32_le(out, price)?;
    out.write_all(&[0x24, 0, 0, 0])?;            // '$'
    write_u32_le(out, if per_layer_settings { 1 } else { 0 })?;
    write_u32_le(out, print_time_sec)?;
    write_u32_le(out, timing.transition_layer_count)?;
    write_u32_le(out, 0)?;                       // TransitionLayerType

    if version >= 516 {
        write_u32_le(out, advanced_mode_flag)?;
    }
    if version >= 517 {
        out.write_all(&[0u8, 0, 0, 0])?;         // Grey + BlurLevel (u16 + u16)
        write_u32_le(out, 0)?;                   // ResinType
    }
    if version >= 518 {
        write_u32_le(out, 0)?;                   // IntelligentMode
    }

    let written = out.stream_position()? - start_pos;
    let expected = header_body_length_for_version(version) as u64;
    debug_assert_eq!(written, expected, "Header body length mismatch for v{version}: wrote {written}, expected {expected}");
    Ok(())
}

#[cfg(test)]
mod header_writer_tests {
    use super::*;
    use super::super::aff_codec::AffRleFormat;

    // pub(super) so sibling #[cfg(test)] modules (table_writer_tests,
    // build_container_tests) can reuse these via `use super::header_writer_tests::*;`
    pub(super) fn dummy_timing() -> AffTimingModel {
        AffTimingModel {
            normal_exposure_sec: 2.0, bottom_exposure_sec: 30.0, bottom_layer_count: 4,
            layer_height_mm: 0.05, wait_time_before_cure_sec: 0.5,
            lift_height_mm: 5.0, lift_speed_mm_s: 2.0, retract_speed_mm_s: 2.0,
            lift_height2_mm: 0.0, lift_speed2_mm_s: 0.0, retract_speed2_mm_s: 0.0,
            bottom_lift_height_mm: 5.0, bottom_lift_speed_mm_s: 2.0, bottom_retract_speed_mm_s: 2.0,
            bottom_lift_height2_mm: 0.0, bottom_lift_speed2_mm_s: 0.0, bottom_retract_speed2_mm_s: 0.0,
            transition_layer_count: 0, anti_alias_level: 1,
        }
    }

    pub(super) fn dummy_build() -> AffBuildModel {
        AffBuildModel {
            machine_name: "Test".into(), display_width_mm: 100.0, display_height_mm: 100.0,
            machine_z_mm: 100.0, pixel_width_um: 47.0, pixel_height_um: 47.0,
            key_suffix: "pm3m".into(), resin_volume_ml: 1000.0, resin_density: 1.2, resin_price: 25.0,
        }
    }

    pub(super) fn dummy_profile() -> AffMachineProfile {
        AffMachineProfile {
            key_suffix: "pm3m", machine_name: "Photon M3 Max", max_version: 516,
            rle_format: AffRleFormat::Pw0,
            display_width_mm: 298.08, display_height_mm: 165.60, machine_z_mm: 300.0,
            default_pixel_um: 47.25, layer_image_format: "pw0Img",
            preview_size: [224, 168], preview2_size: [330, 190],
        }
    }

    #[test]
    fn header_body_v1_is_80_bytes() {
        let mut c = Cursor::new(Vec::new());
        write_header_body(&mut c, 1, 1920, 1080, &dummy_timing(), &dummy_build(), &dummy_profile(),
                          60, 10.0, 12.0, 0.50, false).unwrap();
        assert_eq!(c.into_inner().len(), 80);
    }

    #[test]
    fn header_body_v516_is_84_bytes() {
        let mut c = Cursor::new(Vec::new());
        write_header_body(&mut c, 516, 1920, 1080, &dummy_timing(), &dummy_build(), &dummy_profile(),
                          60, 10.0, 12.0, 0.50, false).unwrap();
        assert_eq!(c.into_inner().len(), 84);
    }

    #[test]
    fn header_body_v517_is_92_bytes() {
        let mut c = Cursor::new(Vec::new());
        write_header_body(&mut c, 517, 1920, 1080, &dummy_timing(), &dummy_build(), &dummy_profile(),
                          60, 10.0, 12.0, 0.50, false).unwrap();
        assert_eq!(c.into_inner().len(), 92);
    }

    #[test]
    fn header_body_v518_is_96_bytes() {
        let mut c = Cursor::new(Vec::new());
        write_header_body(&mut c, 518, 1920, 1080, &dummy_timing(), &dummy_build(), &dummy_profile(),
                          60, 10.0, 12.0, 0.50, false).unwrap();
        assert_eq!(c.into_inner().len(), 96);
    }

    #[test]
    fn header_records_resolution_in_correct_position() {
        let mut c = Cursor::new(Vec::new());
        write_header_body(&mut c, 1, 0xDEAD, 0xBEEF, &dummy_timing(), &dummy_build(), &dummy_profile(),
                          60, 10.0, 12.0, 0.50, false).unwrap();
        let bytes = c.into_inner();
        // ResolutionX at offset 44, ResolutionY at offset 48 (both u32 LE)
        assert_eq!(&bytes[44..48], &0xDEADu32.to_le_bytes());
        assert_eq!(&bytes[48..52], &0xBEEFu32.to_le_bytes());
    }
}

// ── Preview tables (raw RGB565 pixel buffers) ────────────────────────

pub(super) fn write_preview_body<W: Write>(
    out: &mut W,
    width: u32,
    height: u32,
    pixel_data: &[u8],
) -> std::io::Result<()> {
    // Body layout: ResolutionX (u32) + Mark (4 bytes, "x\0\0\0") + ResolutionY (u32) + pixels
    write_u32_le(out, width)?;
    out.write_all(&[b'x', 0, 0, 0])?;
    write_u32_le(out, height)?;
    out.write_all(pixel_data)
}


pub(super) fn write_preview2_body<W: Write>(
    out: &mut W,
    width: u32,
    height: u32,
    pixel_data: &[u8],
) -> std::io::Result<()> {
    // Body layout: ResolutionX (u32) + BackgroundColor1 (u16) + BackgroundColor2 (u16) +
    // ResolutionY (u32) + pixels
    write_u32_le(out, width)?;
    out.write_all(&4293u16.to_le_bytes())?; // BackgroundColor1 default from UVTools
    out.write_all(&0u16.to_le_bytes())?;    // BackgroundColor2
    write_u32_le(out, height)?;
    out.write_all(pixel_data)
}


// ── Extra table (v516+) — TSMC two-stage lift parameters ────────────
//
// Quirk: UVTools serializes all 14 Extra fields (56-byte body) but hard-codes
// the TableLength FIELD to 24 ("Don't know why..."). The firmware reads a
// fixed-size 56-byte struct, so we must emit all 14 fields; only the declared
// length value is the (intentionally wrong) 24. Field ORDER matches UVTools
// exactly, including the deliberately-interleaved RetractSpeed2/Height2 pairs.

/// Value written into the Extra TableLength field (UVTools' hardcoded 24).
pub(super) const EXTRA_DECLARED_LENGTH: u32 = 24;
/// Actual Extra body bytes emitted (all 14 fields).
pub(super) const EXTRA_BODY_BYTES: u32 = 56;

pub(super) fn write_extra_body<W: Write + Seek>(out: &mut W, timing: &AffTimingModel) -> std::io::Result<()> {
    let start = out.stream_position()?;
    // Bottom group
    write_u32_le(out, 2)?;                                  // [0] BottomLiftCount
    write_f32_le(out, timing.bottom_lift_height_mm)?;       // [1] BottomLiftHeight1
    write_f32_le(out, timing.bottom_lift_speed_mm_s)?;      // [2] BottomLiftSpeed1
    write_f32_le(out, timing.bottom_retract_speed2_mm_s)?;  // [3] BottomRetractSpeed2
    write_f32_le(out, timing.bottom_lift_height2_mm)?;      // [4] BottomLiftHeight2
    write_f32_le(out, timing.bottom_lift_speed2_mm_s)?;     // [5] BottomLiftSpeed2
    write_f32_le(out, timing.bottom_retract_speed_mm_s)?;   // [6] BottomRetractSpeed1
    // Normal group
    write_u32_le(out, 2)?;                                  // [7] NormalLiftCount
    write_f32_le(out, timing.lift_height_mm)?;              // [8] LiftHeight1
    write_f32_le(out, timing.lift_speed_mm_s)?;             // [9] LiftSpeed1
    write_f32_le(out, timing.retract_speed2_mm_s)?;         // [10] RetractSpeed2
    write_f32_le(out, timing.lift_height2_mm)?;             // [11] LiftHeight2
    write_f32_le(out, timing.lift_speed2_mm_s)?;            // [12] LiftSpeed2
    write_f32_le(out, timing.retract_speed_mm_s)?;          // [13] RetractSpeed1

    let written = out.stream_position()? - start;
    debug_assert_eq!(written, EXTRA_BODY_BYTES as u64,
        "Extra body length wrong: wrote {written}, expected {EXTRA_BODY_BYTES}");
    Ok(())
}

// ── Machine table (v516+) ───────────────────────────────────────────
//
// Machine has IncludeBaseTableLength=true in UVTools, so its TableLength FIELD
// value is the WHOLE table size (12-byte name + 4-byte length + body). The body
// written after the named header is therefore `table_length - 16`.
//
// UVTools sets MachineSettings.TableLength = 156 for v516/v517 and 224 for v518.
// At TL=156 (< 160) NONE of the SerializeWhen-gated fields (PixelWidthUm etc.)
// are emitted, so the body is exactly the 140 always-present bytes. At TL=224 all
// gated fields emit, giving 140 + 68 = 208 body bytes.

/// TableLength FIELD value for the Machine table (whole table incl. 16-byte base).
pub(super) fn machine_table_length_for_version(v: u16) -> u32 {
    if v >= 518 { 224 } else { 156 }
}

pub(super) fn write_machine_body<W: Write + Seek>(
    out: &mut W,
    version: u16,
    profile: &AffMachineProfile,
    build: &AffBuildModel,
    resolution_x: u32,
    resolution_y: u32,
) -> std::io::Result<()> {
    let start = out.stream_position()?;
    let property_fields: u32 = if version >= 518 { 15 } else if version >= 517 { 7 } else { 1 };

    write_padded_string(out, &build.machine_name, 96)?;             // MachineName
    write_padded_string(out, profile.layer_image_format, 16)?;       // LayerImageFormat
    write_u32_le(out, 16)?;                                          // MaxAntialiasingLevel
    write_u32_le(out, property_fields)?;                             // PropertyFields
    write_f32_le(out, build.display_width_mm)?;                      // DisplayWidth
    write_f32_le(out, build.display_height_mm)?;                     // DisplayHeight
    write_f32_le(out, build.machine_z_mm)?;                          // MachineZ
    write_u32_le(out, profile.max_version as u32)?;                  // MaxFileVersion
    write_u32_le(out, 6506241)?;                                     // MachineBackground
    // 140 bytes so far. v516/v517 stop here (TL=156 -> body=140).

    if version >= 518 {
        // TL=224 -> body=208 -> 68 conditional bytes after the 140 always-present.
        write_f32_le(out, build.pixel_width_um)?;             // PixelWidthUm
        write_f32_le(out, build.pixel_height_um)?;            // PixelHeightUm
        for _ in 0..8 { write_u32_le(out, 0)?; }             // Padding1..8 (32 bytes)
        write_u32_le(out, 1)?;                               // DisplayCount
        write_u32_le(out, 0)?;                               // Padding9
        out.write_all(&(resolution_x as u16).to_le_bytes())?; // ResolutionX (u16)
        out.write_all(&(resolution_y as u16).to_le_bytes())?; // ResolutionY (u16)
        for _ in 0..4 { write_u32_le(out, 0)?; }             // Padding10..13 (16 bytes)
        // 4 + 4 + 32 + 4 + 4 + 2 + 2 + 16 = 68 -> body = 208.
    }

    let written = out.stream_position()? - start;
    let expected = (machine_table_length_for_version(version) - 16) as u64;
    debug_assert_eq!(written, expected,
        "Machine body length wrong for v{version}: wrote {written}, expected {expected}");
    Ok(())
}

// ── Software table (v517+) — no named-table wrapper, just 164 bytes ──

pub(super) const SOFTWARE_TABLE_LENGTH: u32 = 164;

pub(super) fn write_software_body<W: Write + Seek>(out: &mut W) -> std::io::Result<()> {
    let start = out.stream_position()?;
    write_padded_string(out, "DragonFruit", 32)?;                 // SoftwareName
    write_u32_le(out, SOFTWARE_TABLE_LENGTH)?;                    // TableLength (self-describing)
    write_padded_string(out, "1.0.0", 32)?;                       // Version
    let os_tag = format!("rust-{}-{}", std::env::consts::OS, std::env::consts::ARCH);
    write_padded_string(out, &os_tag, 64)?;                         // OperativeSystem
    write_padded_string(out, "3.3-CoreProfile", 32)?;             // OpenGLVersion
    let written = out.stream_position()? - start;
    debug_assert_eq!(written, SOFTWARE_TABLE_LENGTH as u64);
    Ok(())
}

// ── Model table (v517+) — bounding box ──────────────────────────────

pub(super) const MODEL_BODY_LENGTH: u32 = 32;

pub(super) fn write_model_body<W: Write>(
    out: &mut W,
    display_w_mm: f32,
    display_h_mm: f32,
    print_height_mm: f32,
) -> std::io::Result<()> {
    let half_w = display_w_mm / 2.0;
    let half_h = display_h_mm / 2.0;
    write_f32_le(out, -half_w)?;     // MinX
    write_f32_le(out, -half_h)?;     // MinY
    write_f32_le(out, 0.0)?;         // MinZ
    write_f32_le(out, half_w)?;      // MaxX
    write_f32_le(out, half_h)?;      // MaxY
    write_f32_le(out, print_height_mm)?; // MaxZ
    write_u32_le(out, 0)?;           // SupportsEnabled
    write_f32_le(out, 0.0)?;         // SupportsDensity
    Ok(())
}

// ── LayerImageColorTable (v515+) ────────────────────────────────────

#[allow(dead_code)]
pub(super) const COLOR_TABLE_LENGTH: u32 = 4 + 4 + 16 + 4; // UseFullGrey(u32) + GreyMaxCount(u32) + Grey[16] + Unknown(u32)

pub(super) fn write_color_table_body<W: Write>(out: &mut W, aa_level: u32) -> std::io::Result<()> {
    let count: u32 = aa_level.clamp(1, 16);
    write_u32_le(out, 0)?;       // UseFullGreyscale
    write_u32_le(out, count)?;   // GreyMaxCount
    let increment = 255.0f32 / count as f32;
    let mut grey_bytes = [0u8; 16];
    for i in 0..16u32 {
        if i < count {
            grey_bytes[i as usize] = ((i as f32 + 1.0) * increment).min(255.0) as u8;
        } else {
            grey_bytes[i as usize] = 255;
        }
    }
    out.write_all(&grey_bytes)?;
    write_u32_le(out, 0)?;       // Unknown
    Ok(())
}

#[cfg(test)]
mod table_writer_tests {
    use super::*;
    use super::header_writer_tests::*; // reuse dummy_* helpers

    fn dummy_t() -> AffTimingModel { dummy_timing() }
    fn dummy_b() -> AffBuildModel { dummy_build() }
    fn dummy_p() -> AffMachineProfile { dummy_profile() }

    #[test]
    fn extra_body_is_56_bytes() {
        // All 14 Extra fields emit even though TableLength declares 24.
        let mut c = Cursor::new(Vec::new());
        write_extra_body(&mut c, &dummy_t()).unwrap();
        assert_eq!(c.into_inner().len(), EXTRA_BODY_BYTES as usize);
    }

    #[test]
    fn machine_body_v516_is_140_bytes() {
        // IncludeBaseTableLength=true: TableLength field = 156 (whole), body = 156 - 16 = 140.
        let mut c = Cursor::new(Vec::new());
        write_machine_body(&mut c, 516, &dummy_p(), &dummy_b(), 6480, 3600).unwrap();
        assert_eq!(c.into_inner().len(), 140);
    }

    #[test]
    fn machine_body_v518_is_208_bytes() {
        // TableLength field = 224 (whole), body = 224 - 16 = 208.
        let mut c = Cursor::new(Vec::new());
        write_machine_body(&mut c, 518, &dummy_p(), &dummy_b(), 6480, 3600).unwrap();
        assert_eq!(c.into_inner().len(), 208);
    }

    #[test]
    fn software_body_is_164_bytes() {
        let mut c = Cursor::new(Vec::new());
        write_software_body(&mut c).unwrap();
        assert_eq!(c.into_inner().len(), SOFTWARE_TABLE_LENGTH as usize);
    }

    #[test]
    fn model_body_is_32_bytes() {
        let mut c = Cursor::new(Vec::new());
        write_model_body(&mut c, 100.0, 60.0, 50.0).unwrap();
        assert_eq!(c.into_inner().len(), MODEL_BODY_LENGTH as usize);
    }

    #[test]
    fn color_table_is_28_bytes() {
        let mut c = Cursor::new(Vec::new());
        write_color_table_body(&mut c, 1).unwrap();
        assert_eq!(c.into_inner().len(), COLOR_TABLE_LENGTH as usize);
    }
}

// ── LayerDef + SubLayerDef ──────────────────────────────────────────

pub(super) const LAYER_DEF_ENTRY_SIZE: u32 = 32; // C# ClassSize constant
pub(super) const SUB_LAYER_DEF_ENTRY_SIZE: u32 = 4 + 4 + 4 + 4*8; // DataAddress(u32) + DataLength(u32) + NonZeroPixels(u32) + 8 floats

/// One layer's per-layer parameters as written into LayerDef entries.
pub(super) struct AffLayerEntry {
    pub data_address: u32,
    pub data_length: u32,
    pub lift_height_mm: f32,
    pub lift_speed_mm_s: f32,
    pub exposure_time_sec: f32,
    pub layer_height_mm: f32,
    pub non_zero_pixel_count: u32,
}

pub(super) fn write_layer_def_body<W: Write>(
    out: &mut W,
    layers: impl ExactSizeIterator<Item = AffLayerEntry>,
) -> std::io::Result<()> {
    write_u32_le(out, layers.len() as u32)?;
    for entry in layers {
        write_u32_le(out, entry.data_address)?;
        write_u32_le(out, entry.data_length)?;
        write_f32_le(out, entry.lift_height_mm)?;
        write_f32_le(out, entry.lift_speed_mm_s)?;
        write_f32_le(out, entry.exposure_time_sec)?;
        write_f32_le(out, entry.layer_height_mm)?;
        write_u32_le(out, entry.non_zero_pixel_count)?;
        write_u32_le(out, 0)?; // Padding1
    }
    Ok(())
}

pub(super) fn write_sub_layer_def_body<W: Write>(
    out: &mut W,
    layers: impl ExactSizeIterator<Item = AffLayerEntry>,
) -> std::io::Result<()> {
    write_u32_le(out, layers.len() as u32)?;
    write_u32_le(out, 1)?; // Index (always 1 per UVTools default)
    for entry in layers {
        write_u32_le(out, entry.data_address)?;
        write_u32_le(out, entry.data_length)?;
        write_u32_le(out, entry.non_zero_pixel_count)?;
        for _ in 0..8 { write_f32_le(out, 0.0)?; } // 8 padding floats
    }
    Ok(())
}

// ── FileMark ────────────────────────────────────────────────────────
//
// Byte counts derived field-by-field from UVTools FileMark (SerializeWhen
// attributes). Always present: Mark(12) + Version + NumberOfTables +
// HeaderAddress + SoftwareAddress + PreviewAddress + LayerImageColorTableAddress
// + LayerDefinitionAddress + ExtraAddress + LayerImageAddress = 12 + 9*4 = 48.
// v516 adds MachineAddress (+4=52); v517 adds ModelAddress (+4=56);
// v518 adds SubLayerDefinitionAddress + Preview2Address (+8=64).
pub(super) const FILEMARK_LEN_V1: u32 = 48;
pub(super) const FILEMARK_LEN_V515: u32 = 48;
pub(super) const FILEMARK_LEN_V516: u32 = 52;
pub(super) const FILEMARK_LEN_V517: u32 = 56;
pub(super) const FILEMARK_LEN_V518: u32 = 64;

pub(super) fn filemark_length_for_version(v: u16) -> u32 {
    match v {
        518 => FILEMARK_LEN_V518,
        517 => FILEMARK_LEN_V517,
        516 => FILEMARK_LEN_V516,
        515 => FILEMARK_LEN_V515,
        _ => FILEMARK_LEN_V1,
    }
}

#[derive(Default, Debug, Clone, Copy)]
pub(super) struct FileMarkAddresses {
    pub header: u32,
    pub software: u32,
    pub preview: u32,
    pub layer_image_color_table: u32,
    pub layer_definition: u32,
    pub extra: u32,
    pub machine: u32,
    pub layer_image: u32,
    pub model: u32,
    pub sub_layer_definition: u32,
    pub preview2: u32,
}

pub(super) fn number_of_tables_for_version(v: u16) -> u32 {
    match v {
        518 => 11,
        517 => 9,
        516 => 8,
        515 => 5,
        _ => 4,
    }
}

pub(super) fn write_filemark<W: Write + Seek>(
    out: &mut W,
    version: u16,
    addresses: &FileMarkAddresses,
) -> std::io::Result<()> {
    let start = out.stream_position()?;
    write_padded_string(out, "ANYCUBIC", 12)?;                // Mark
    write_u32_le(out, version as u32)?;                       // Version
    write_u32_le(out, number_of_tables_for_version(version))?; // NumberOfTables
    write_u32_le(out, addresses.header)?;                     // HeaderAddress
    write_u32_le(out, addresses.software)?;                   // SoftwareAddress
    write_u32_le(out, addresses.preview)?;                    // PreviewAddress
    write_u32_le(out, addresses.layer_image_color_table)?;    // LayerImageColorTableAddress
    write_u32_le(out, addresses.layer_definition)?;           // LayerDefinitionAddress
    write_u32_le(out, addresses.extra)?;                      // ExtraAddress
    if version >= 516 {
        write_u32_le(out, addresses.machine)?;                // MachineAddress
    }
    write_u32_le(out, addresses.layer_image)?;                // LayerImageAddress
    if version >= 517 {
        write_u32_le(out, addresses.model)?;                  // ModelAddress
    }
    if version >= 518 {
        write_u32_le(out, addresses.sub_layer_definition)?;   // SubLayerDefinitionAddress
        write_u32_le(out, addresses.preview2)?;               // Preview2Address
    }
    let written = out.stream_position()? - start;
    let expected = filemark_length_for_version(version) as u64;
    debug_assert_eq!(written, expected,
        "FileMark length wrong for v{version}: wrote {written}, expected {expected}");
    Ok(())
}

#[cfg(test)]
mod layer_and_filemark_tests {
    use super::*;

    #[test]
    fn layer_def_entry_is_32_bytes() {
        let entry = AffLayerEntry {
            data_address: 100, data_length: 50,
            lift_height_mm: 5.0, lift_speed_mm_s: 2.0,
            exposure_time_sec: 2.0, layer_height_mm: 0.05,
            non_zero_pixel_count: 12345,
        };
        let mut c = Cursor::new(Vec::new());
        write_layer_def_body(&mut c, std::iter::once(entry)).unwrap();
        // 4 (LayerCount) + 32 (entry)
        assert_eq!(c.into_inner().len(), 36);
    }

    #[test]
    fn sub_layer_def_entry_is_44_bytes() {
        let entry = AffLayerEntry {
            data_address: 100, data_length: 50,
            lift_height_mm: 0.0, lift_speed_mm_s: 0.0,
            exposure_time_sec: 0.0, layer_height_mm: 0.0,
            non_zero_pixel_count: 0,
        };
        let mut c = Cursor::new(Vec::new());
        write_sub_layer_def_body(&mut c, std::iter::once(entry)).unwrap();
        // 4 (LayerCount) + 4 (Index) + 44 (entry: 3*u32 + 8*f32)
        assert_eq!(c.into_inner().len(), 52);
    }

    #[test]
    fn filemark_v1_is_48_bytes_with_ANYCUBIC_mark() {
        let mut c = Cursor::new(Vec::new());
        write_filemark(&mut c, 1, &FileMarkAddresses::default()).unwrap();
        let bytes = c.into_inner();
        assert_eq!(bytes.len(), 48);
        assert_eq!(&bytes[..8], b"ANYCUBIC");
        assert_eq!(&bytes[12..16], &1u32.to_le_bytes()); // Version
        assert_eq!(&bytes[16..20], &4u32.to_le_bytes()); // NumberOfTables
    }

    #[test]
    fn filemark_v518_is_64_bytes_with_11_tables() {
        let mut c = Cursor::new(Vec::new());
        write_filemark(&mut c, 518, &FileMarkAddresses::default()).unwrap();
        let bytes = c.into_inner();
        assert_eq!(bytes.len(), 64);
        assert_eq!(&bytes[16..20], &11u32.to_le_bytes());
    }
}

// ═══════════════════════════════════════════════════════════════════
//  build_aff_container — full file assembly
// ═══════════════════════════════════════════════════════════════════

use super::aff_preview::build_preview_rgb565;
use crate::engine::SlicerV3Error;
use crate::types::SliceJobV3;

pub(super) fn build_aff_container(
    job: &SliceJobV3,
    timing: &AffTimingModel,
    build: &AffBuildModel,
    profile: &AffMachineProfile,
    layers: &[StoredLayer],
    payloads: &mut File,
) -> Result<Vec<u8>, SlicerV3Error> {
    let output = write_aff_container(job, timing, build, profile, layers, payloads, Cursor::new(Vec::new()))?;
    Ok(output.into_inner())
}

/// Writes an AFF file from sorted layer metadata and scratch-file extents.
/// The output is written from offset zero and returned positioned at its end.
pub(super) fn write_aff_container<W: Write + Seek>(
    job: &SliceJobV3,
    timing: &AffTimingModel,
    build: &AffBuildModel,
    profile: &AffMachineProfile,
    layers: &[StoredLayer],
    payloads: &mut File,
    mut output: W,
) -> Result<W, SlicerV3Error> {
    let version = profile.max_version;
    let overflow = || io::Error::new(io::ErrorKind::InvalidInput, "AFF file exceeds 32-bit format limits");
    let layer_count = u32::try_from(layers.len()).map_err(|_| overflow())?;
    let layer_def_len = layer_count.checked_mul(LAYER_DEF_ENTRY_SIZE)
        .and_then(|n| n.checked_add(4)).ok_or_else(overflow)?;
    let sub_layer_def_len = if version >= 518 {
        layer_count.checked_mul(SUB_LAYER_DEF_ENTRY_SIZE)
            .and_then(|n| n.checked_add(8)).ok_or_else(overflow)?
    } else { 0 };
    let preview_len = |size: [u32; 2]| -> io::Result<u32> {
        size[0].checked_mul(size[1]).and_then(|n| n.checked_mul(2))
            .and_then(|n| n.checked_add(28)).ok_or_else(overflow)
    };
    let preview_total = preview_len(profile.preview_size)?;
    let preview2_total = if version >= 518 { preview_len(profile.preview2_size)? } else { 0 };

    // Calculate absolute addresses before writing; no layer-sized reservations
    // or per-job table/payload buffers are needed.
    let mut end = filemark_length_for_version(version);
    let mut reserve = |length: u64| -> io::Result<u32> {
        let start = end;
        let length = u32::try_from(length).map_err(|_| overflow())?;
        end = end.checked_add(length).ok_or_else(overflow)?;
        Ok(start)
    };
    let mut addresses = FileMarkAddresses::default();
    addresses.header = reserve(16 + header_body_length_for_version(version) as u64)?;
    addresses.preview = reserve(preview_total as u64)?;
    if version >= 515 {
        addresses.layer_image_color_table = reserve(COLOR_TABLE_LENGTH as u64)?;
    }
    addresses.layer_definition = reserve(16 + layer_def_len as u64)?;
    if version >= 516 {
        // Extra declares 24 bytes but actually serializes its 56-byte body.
        addresses.extra = reserve(16 + EXTRA_BODY_BYTES as u64)?;
        addresses.machine = reserve(machine_table_length_for_version(version) as u64)?;
    }
    if version >= 517 {
        addresses.software = reserve(SOFTWARE_TABLE_LENGTH as u64)?;
        addresses.model = reserve(16 + MODEL_BODY_LENGTH as u64)?;
    }
    if version >= 518 {
        addresses.sub_layer_definition = reserve(16 + sub_layer_def_len as u64)?;
        addresses.preview2 = reserve(preview2_total as u64)?;
    }
    addresses.layer_image = reserve(0)?;
    for layer in layers {
        reserve(layer.encoded_len)?;
    }

    output.seek(SeekFrom::Start(0))?;
    write_filemark(&mut output, version, &addresses)?;
    write_named_table_header(&mut output, "HEADER", header_body_length_for_version(version))?;
    let print_time_sec = compute_print_time_sec(timing, layer_count) as u32;
    let volume_ml = compute_volume_ml(timing, build, job, layers);
    let weight_g = volume_ml * build.resin_density;
    let cost = if build.resin_volume_ml > 0.0 {
        (volume_ml / build.resin_volume_ml) * build.resin_price
    } else { 0.0 };
    write_header_body(
        &mut output, version, job.source_width_px, job.source_height_px,
        timing, build, profile, print_time_sec, volume_ml, weight_g, cost, false,
    )?;

    // Preview TableLength includes the named-table header.
    let [prev_w, prev_h] = profile.preview_size;
    let preview_pixels = build_preview_rgb565(job.export_thumbnail_png_base64.as_deref(), prev_w, prev_h);
    write_named_table_header(&mut output, "PREVIEW", preview_total)?;
    write_preview_body(&mut output, prev_w, prev_h, &preview_pixels)?;
    if version >= 515 {
        // This color table is serialized raw, without a named-table wrapper.
        write_color_table_body(&mut output, timing.anti_alias_level)?;
    }
    write_named_table_header(&mut output, "LAYERDEF", layer_def_len)?;
    write_layer_def_body(&mut output, aff_layer_entries(layers, timing, addresses.layer_image))?;
    if version >= 516 {
        write_named_table_header(&mut output, "EXTRA", EXTRA_DECLARED_LENGTH)?;
        write_extra_body(&mut output, timing)?;
        write_named_table_header(&mut output, "MACHINE", machine_table_length_for_version(version))?;
        write_machine_body(&mut output, version, profile, build, job.source_width_px, job.source_height_px)?;
    }
    if version >= 517 {
        write_software_body(&mut output)?;
        write_named_table_header(&mut output, "MODEL", MODEL_BODY_LENGTH + 16)?;
        let print_height = timing.layer_height_mm * layer_count as f32;
        write_model_body(&mut output, build.display_width_mm, build.display_height_mm, print_height)?;
    }
    if version >= 518 {
        write_named_table_header(&mut output, "SUBIMGS", sub_layer_def_len + 16)?;
        write_sub_layer_def_body(&mut output, aff_layer_entries(layers, timing, addresses.layer_image))?;
        let [p2w, p2h] = profile.preview2_size;
        let p2_pixels = build_preview_rgb565(job.export_thumbnail_png_base64.as_deref(), p2w, p2h);
        write_named_table_header(&mut output, "PREVIEW2", preview2_total)?;
        write_preview2_body(&mut output, p2w, p2h, &p2_pixels)?;
    }

    for layer in layers {
        payloads.seek(SeekFrom::Start(layer.offset))?;
        let copied = io::copy(&mut (&mut *payloads).take(layer.encoded_len), &mut output)?;
        if copied != layer.encoded_len {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, format!(
                "AFF layer {} scratch extent is truncated: expected {} bytes, read {}",
                layer.index, layer.encoded_len, copied,
            )).into());
        }
    }
    Ok(output)
}

fn aff_layer_entries<'a>(
    layers: &'a [StoredLayer],
    timing: &'a AffTimingModel,
    mut data_address: u32,
) -> impl ExactSizeIterator<Item = AffLayerEntry> + 'a {
    let transition_start = timing.bottom_layer_count as u64;
    let transition_end = transition_start + timing.transition_layer_count as u64;
    layers.iter().map(move |layer| {
        let layer_idx = layer.index as u64;
        let is_bottom = layer_idx < transition_start;
        let exposure = if is_bottom {
            timing.bottom_exposure_sec
        } else if layer_idx < transition_end && timing.transition_layer_count > 0 {
            let progress = (layer_idx - transition_start) as f32 / timing.transition_layer_count as f32;
            timing.bottom_exposure_sec + (timing.normal_exposure_sec - timing.bottom_exposure_sec) * progress
        } else {
            timing.normal_exposure_sec
        };
        // The layout pass checked every payload length and cumulative address.
        let data_length = layer.encoded_len as u32;
        let entry = AffLayerEntry {
            data_address,
            data_length,
            lift_height_mm: if is_bottom { timing.bottom_lift_height_mm + timing.bottom_lift_height2_mm }
                else { timing.lift_height_mm + timing.lift_height2_mm },
            lift_speed_mm_s: if is_bottom { timing.bottom_lift_speed_mm_s } else { timing.lift_speed_mm_s },
            exposure_time_sec: exposure,
            layer_height_mm: timing.layer_height_mm, // relative thickness, not absolute Z
            non_zero_pixel_count: layer.non_zero_pixel_count,
        };
        data_address += data_length;
        entry
    })
}

// ── Cost / time helpers ─────────────────────────────────────────────

fn compute_print_time_sec(timing: &AffTimingModel, layer_count: u32) -> f32 {
    let bottom = timing.bottom_layer_count.min(layer_count);
    let normal = layer_count.saturating_sub(bottom);

    let bottom_lift_total = timing.bottom_lift_height_mm + timing.bottom_lift_height2_mm;
    let normal_lift_total = timing.lift_height_mm + timing.lift_height2_mm;

    let bottom_motion_sec =
        if timing.bottom_lift_speed_mm_s > 0.0 { bottom_lift_total / timing.bottom_lift_speed_mm_s } else { 0.0 } +
        if timing.bottom_retract_speed_mm_s > 0.0 { bottom_lift_total / timing.bottom_retract_speed_mm_s } else { 0.0 };
    let normal_motion_sec =
        if timing.lift_speed_mm_s > 0.0 { normal_lift_total / timing.lift_speed_mm_s } else { 0.0 } +
        if timing.retract_speed_mm_s > 0.0 { normal_lift_total / timing.retract_speed_mm_s } else { 0.0 };

    (bottom as f32) * (timing.bottom_exposure_sec + timing.wait_time_before_cure_sec + bottom_motion_sec) +
    (normal as f32) * (timing.normal_exposure_sec + timing.wait_time_before_cure_sec + normal_motion_sec)
}

fn compute_volume_ml(
    timing: &AffTimingModel,
    build: &AffBuildModel,
    job: &SliceJobV3,
    layers: &[StoredLayer],
) -> f32 {
    let pixel_area_mm2 = (build.display_width_mm / job.source_width_px as f32)
                       * (build.display_height_mm / job.source_height_px as f32);
    let total_mm3: f32 = layers
        .iter()
        .map(|l| l.non_zero_pixel_count as f32 * pixel_area_mm2 * timing.layer_height_mm)
        .sum();
    total_mm3 / 1000.0
}

#[cfg(test)]
mod build_container_tests {
    use super::*;
    use super::header_writer_tests::dummy_timing;
    use super::super::aff_codec::AffRleFormat;

    fn make_job() -> SliceJobV3 {
        SliceJobV3 {
            output_format: ".aff".to_string(),
            format_version: None,
            source_width_px: 4,
            source_height_px: 4,
            width_px: 4,
            height_px: 4,
            build_width_mm: 10.0,
            build_depth_mm: 20.0,
            layer_height_mm: 0.05,
            total_layers: 2,
            export_thumbnail_png_base64: None,
            png_compression_strategy: "balanced".to_string(),
            container_compression_level: 2,
            anti_aliasing_level: "Off".to_string(),
            aa_on_supports: false,
            minimum_aa_alpha_percent: 35.0,
            mirror_x: false,
            mirror_y: false,
            triangles_xyz: vec![],
            metadata_json: "{}".to_string(),
            x_packing_mode: "none".to_string(),
            ..Default::default()
        }
    }

    fn pm3m_profile() -> AffMachineProfile {
        AffMachineProfile {
            key_suffix: "pm3m", machine_name: "Photon M3 Max", max_version: 516,
            rle_format: AffRleFormat::Pw0,
            display_width_mm: 298.08, display_height_mm: 165.60, machine_z_mm: 300.0,
            default_pixel_um: 47.25, layer_image_format: "pw0Img",
            preview_size: [224, 168], preview2_size: [330, 190],
        }
    }

    fn pm3m_build() -> AffBuildModel {
        AffBuildModel {
            machine_name: "Photon M3 Max".into(),
            display_width_mm: 298.08, display_height_mm: 165.60,
            machine_z_mm: 300.0, pixel_width_um: 46.0, pixel_height_um: 46.0,
            key_suffix: "pm3m".into(),
            resin_volume_ml: 1000.0, resin_density: 1.2, resin_price: 25.0,
        }
    }

    fn with_payloads<T>(bytes: &[u8], f: impl FnOnce(&mut File) -> T) -> T {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "anycubic-aff-layout-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed),
        ));
        let mut file = std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(&path).unwrap();
        file.write_all(bytes).unwrap();
        let result = f(&mut file);
        drop(file);
        std::fs::remove_file(path).unwrap();
        result
    }

    fn one_layer_container(profile: &AffMachineProfile) -> Vec<u8> {
        let layers = [StoredLayer { index: 0, offset: 0, encoded_len: 2, non_zero_pixel_count: 0 }];
        with_payloads(&[0x00, 16], |payloads| {
            build_aff_container(&make_job(), &dummy_timing(), &pm3m_build(), profile, &layers, payloads).unwrap()
        })
    }

    #[test]
    fn pm3m_v516_container_starts_with_anycubic_mark() {
        let bytes = one_layer_container(&pm3m_profile());
        assert_eq!(&bytes[..8], b"ANYCUBIC");
    }

    #[test]
    fn pm3m_v516_filemark_version_is_516() {
        let bytes = one_layer_container(&pm3m_profile());
        assert_eq!(&bytes[12..16], &516u32.to_le_bytes());
    }

    #[test]
    fn pm3m_v516_contains_header_extra_machine_table_names() {
        let bytes = one_layer_container(&pm3m_profile());
        assert!(bytes.windows(6).any(|w| w == b"HEADER"));
        assert!(bytes.windows(5).any(|w| w == b"EXTRA"));
        assert!(bytes.windows(7).any(|w| w == b"MACHINE"));
        assert!(bytes.windows(7).any(|w| w == b"PREVIEW"));
        assert!(bytes.windows(8).any(|w| w == b"LAYERDEF"));
    }

    #[test]
    fn header_address_points_to_HEADER_table() {
        let bytes = one_layer_container(&pm3m_profile());
        let header_addr = u32::from_le_bytes(bytes[20..24].try_into().unwrap()) as usize;
        assert_eq!(&bytes[header_addr..header_addr + 6], b"HEADER");
    }

    #[test]
    fn v518_container_includes_subimgs_and_preview2() {
        let mut profile = pm3m_profile();
        profile.max_version = 518;
        let bytes = one_layer_container(&profile);
        assert!(bytes.windows(7).any(|w| w == b"SUBIMGS"));
        assert!(bytes.windows(8).any(|w| w == b"PREVIEW2"));
    }

    fn u32_at(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    #[test]
    fn file_writer_preserves_offsets_and_layer_order_for_all_aff_versions() {
        for (version, expected_image_address) in [(1, 260), (515, 288), (516, 524), (517, 748), (518, 970)] {
            let mut profile = pm3m_profile();
            profile.max_version = version;
            profile.preview_size = [2, 1];
            profile.preview2_size = [1, 1];
            if version == 1 {
                profile.rle_format = AffRleFormat::Pws;
                profile.layer_image_format = "pwsImg";
            }
            let layers = [
                StoredLayer { index: 0, offset: 3, encoded_len: 2, non_zero_pixel_count: 4 },
                StoredLayer { index: 1, offset: 0, encoded_len: 3, non_zero_pixel_count: 7 },
            ];
            let mut timing = dummy_timing();
            timing.bottom_layer_count = 1;
            let bytes = with_payloads(&[9, 8, 7, 1, 2, 99], |payloads| {
                with_payloads(&[], |output| {
                    let output = write_aff_container(&make_job(), &timing, &pm3m_build(), &profile, &layers, payloads, output).unwrap();
                    assert_eq!(output.stream_position().unwrap(), expected_image_address as u64 + 5);
                    output.seek(SeekFrom::Start(0)).unwrap();
                    let mut bytes = Vec::new();
                    output.read_to_end(&mut bytes).unwrap();
                    bytes
                })
            });
            let image_field = if version >= 516 { 48 } else { 44 };
            assert_eq!(u32_at(&bytes, image_field), expected_image_address);
            assert_eq!(&bytes[expected_image_address as usize..], &[1, 2, 9, 8, 7]);
            let layer_def = u32_at(&bytes, 36) as usize;
            assert_eq!(&bytes[layer_def..layer_def + 8], b"LAYERDEF");
            assert_eq!(u32_at(&bytes, layer_def + 12), 68);
            assert_eq!(u32_at(&bytes, layer_def + 16), 2);
            for (index, length, non_zero, exposure) in [(0, 2, 4, 30.0f32), (1, 3, 7, 2.0f32)] {
                let entry = layer_def + 20 + index * 32;
                let address = expected_image_address + if index == 0 { 0 } else { 2 };
                assert_eq!(u32_at(&bytes, entry), address);
                assert_eq!(u32_at(&bytes, entry + 4), length);
                assert_eq!(u32_at(&bytes, entry + 16), exposure.to_bits());
                assert_eq!(u32_at(&bytes, entry + 20), timing.layer_height_mm.to_bits());
                assert_eq!(u32_at(&bytes, entry + 24), non_zero);
                if version == 518 {
                    let sub_def = u32_at(&bytes, 56) as usize;
                    assert_eq!(u32_at(&bytes, sub_def + 12), 112);
                    let sub_entry = sub_def + 24 + index * 44;
                    assert_eq!(u32_at(&bytes, sub_entry), address);
                    assert_eq!(u32_at(&bytes, sub_entry + 4), length);
                    assert_eq!(u32_at(&bytes, sub_entry + 8), non_zero);
                }
            }
        }
    }

    #[test]
    fn truncated_scratch_extent_is_reported_instead_of_shortening_layer() {
        let layers = [StoredLayer { index: 0, offset: 1, encoded_len: 2, non_zero_pixel_count: 1 }];
        with_payloads(&[9, 8], |payloads| {
            let error = build_aff_container(&make_job(), &dummy_timing(), &pm3m_build(), &pm3m_profile(), &layers, payloads).unwrap_err();
            assert!(error.to_string().contains("AFF layer 0 scratch extent is truncated"));
        });
    }

    #[test]
    fn payload_overflow_is_rejected_before_writing_output() {
        let layers = [StoredLayer { index: 0, offset: 0, encoded_len: u32::MAX as u64, non_zero_pixel_count: 0 }];
        with_payloads(&[], |payloads| {
            let mut output = Cursor::new(Vec::new());
            let error = write_aff_container(&make_job(), &dummy_timing(), &pm3m_build(), &pm3m_profile(), &layers, payloads, &mut output).unwrap_err();
            assert!(error.to_string().contains("AFF file exceeds 32-bit format limits"));
            assert!(output.into_inner().is_empty());
        });
    }
}
