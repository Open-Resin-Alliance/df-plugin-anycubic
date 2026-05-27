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

pub(super) fn write_header_body(
    out: &mut Cursor<Vec<u8>>,
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
    let start_pos = out.position();

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
    write_u32_le(out, 1)?;                       // AntiAliasing
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

    let written = out.position() - start_pos;
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
            transition_layer_count: 0, anti_alias_level: 1, twostage: false,
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

pub(super) fn write_preview_body(
    out: &mut Cursor<Vec<u8>>,
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

pub(super) fn preview_body_length(width: u32, height: u32) -> u32 {
    12 + (width * height * 2)
}

pub(super) fn write_preview2_body(
    out: &mut Cursor<Vec<u8>>,
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

pub(super) fn preview2_body_length(width: u32, height: u32) -> u32 {
    12 + (width * height * 2)
}

#[cfg(test)]
mod preview_writer_tests {
    use super::*;

    #[test]
    fn preview_body_length_matches_written_size() {
        let pixels = vec![0u8; 224 * 168 * 2];
        let mut c = Cursor::new(Vec::new());
        write_preview_body(&mut c, 224, 168, &pixels).unwrap();
        assert_eq!(c.into_inner().len() as u32, preview_body_length(224, 168));
    }

    #[test]
    fn preview2_body_length_matches_written_size() {
        let pixels = vec![0u8; 330 * 190 * 2];
        let mut c = Cursor::new(Vec::new());
        write_preview2_body(&mut c, 330, 190, &pixels).unwrap();
        assert_eq!(c.into_inner().len() as u32, preview2_body_length(330, 190));
    }
}

// ── Extra table (v516+) — TSMC two-stage lift parameters ────────────

pub(super) const EXTRA_BODY_LENGTH: u32 = 24; // C# always serializes 24 bytes regardless of struct size

pub(super) fn write_extra_body(out: &mut Cursor<Vec<u8>>, timing: &AffTimingModel) -> std::io::Result<()> {
    let start = out.position();
    write_u32_le(out, 2)?;                                  // BottomLiftCount
    write_f32_le(out, timing.bottom_lift_height_mm)?;       // BottomLiftHeight1
    write_f32_le(out, timing.bottom_lift_speed_mm_s)?;      // BottomLiftSpeed1
    write_f32_le(out, timing.bottom_retract_speed2_mm_s)?;  // BottomRetractSpeed2 (intentional ordering!)
    write_f32_le(out, timing.bottom_lift_height2_mm)?;      // BottomLiftHeight2
    // Total so far: 4 + 4*4 = 20 bytes. C# TableLength is 24. The remaining 4 bytes are
    // BottomLiftSpeed2 (truncated by the manually-set TableLength=24). We pad to match:
    write_f32_le(out, timing.bottom_lift_speed2_mm_s)?;     // BottomLiftSpeed2 -> reaches 24 bytes

    let written = out.position() - start;
    debug_assert_eq!(written, EXTRA_BODY_LENGTH as u64,
        "Extra body length wrong: wrote {written}, expected {EXTRA_BODY_LENGTH}");
    Ok(())
}

// ── Machine table (v516+) ───────────────────────────────────────────

pub(super) fn machine_body_length_for_version(v: u16) -> u32 {
    if v >= 518 { 224 } else { 156 }
}

pub(super) fn write_machine_body(
    out: &mut Cursor<Vec<u8>>,
    version: u16,
    profile: &AffMachineProfile,
    build: &AffBuildModel,
    resolution_x: u32,
    resolution_y: u32,
) -> std::io::Result<()> {
    let start = out.position();
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
    // Position now: 96 + 16 + 4 + 4 + 4 + 4 + 4 + 4 + 4 = 140 bytes from start.
    // v516 TableLength is 156 -> need 16 more bytes:
    write_f32_le(out, build.pixel_width_um)?;    // PixelWidthUm
    write_f32_le(out, build.pixel_height_um)?;   // PixelHeightUm
    write_u32_le(out, 0)?;                       // Padding1
    write_u32_le(out, 0)?;                       // Padding2

    if version >= 518 {
        // v518 TableLength=224: need 224 - 156 = 68 more bytes
        for _ in 0..6 { write_u32_le(out, 0)?; }              // Padding3..8 (24 bytes)
        write_u32_le(out, 1)?;                                // DisplayCount
        write_u32_le(out, 0)?;                                // Padding9
        out.write_all(&(resolution_x as u16).to_le_bytes())?;  // ResolutionX
        out.write_all(&(resolution_y as u16).to_le_bytes())?;  // ResolutionY
        for _ in 0..4 { write_u32_le(out, 0)?; }              // Padding10..13 (16 bytes)
        // 24 + 4 + 4 + 2 + 2 + 16 = 52, but we need 68. Add more padding:
        for _ in 0..4 { write_u32_le(out, 0)?; }              // extra padding -> 68
    }

    let written = out.position() - start;
    let expected = machine_body_length_for_version(version) as u64;
    debug_assert_eq!(written, expected,
        "Machine body length wrong for v{version}: wrote {written}, expected {expected}");
    Ok(())
}

// ── Software table (v517+) — no named-table wrapper, just 164 bytes ──

pub(super) const SOFTWARE_TABLE_LENGTH: u32 = 164;

pub(super) fn write_software_body(out: &mut Cursor<Vec<u8>>) -> std::io::Result<()> {
    let start = out.position();
    write_padded_string(out, "DragonFruit", 32)?;                 // SoftwareName
    write_u32_le(out, SOFTWARE_TABLE_LENGTH)?;                    // TableLength (self-describing)
    write_padded_string(out, "1.0.0", 32)?;                       // Version
    write_padded_string(out, "rust-windows", 64)?;                // OperativeSystem
    write_padded_string(out, "3.3-CoreProfile", 32)?;             // OpenGLVersion
    let written = out.position() - start;
    debug_assert_eq!(written, SOFTWARE_TABLE_LENGTH as u64);
    Ok(())
}

// ── Model table (v517+) — bounding box ──────────────────────────────

pub(super) const MODEL_BODY_LENGTH: u32 = 32;

pub(super) fn write_model_body(
    out: &mut Cursor<Vec<u8>>,
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

pub(super) const COLOR_TABLE_LENGTH: u32 = 4 + 4 + 16 + 4; // UseFullGrey(u32) + GreyMaxCount(u32) + Grey[16] + Unknown(u32)

pub(super) fn write_color_table_body(out: &mut Cursor<Vec<u8>>, aa_level: u32) -> std::io::Result<()> {
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
    fn extra_body_is_24_bytes() {
        let mut c = Cursor::new(Vec::new());
        write_extra_body(&mut c, &dummy_t()).unwrap();
        assert_eq!(c.into_inner().len(), EXTRA_BODY_LENGTH as usize);
    }

    #[test]
    fn machine_body_v516_is_156_bytes() {
        let mut c = Cursor::new(Vec::new());
        write_machine_body(&mut c, 516, &dummy_p(), &dummy_b(), 6480, 3600).unwrap();
        assert_eq!(c.into_inner().len(), 156);
    }

    #[test]
    fn machine_body_v518_is_224_bytes() {
        let mut c = Cursor::new(Vec::new());
        write_machine_body(&mut c, 518, &dummy_p(), &dummy_b(), 6480, 3600).unwrap();
        assert_eq!(c.into_inner().len(), 224);
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

pub(super) fn layer_def_body_length(layer_count: u32) -> u32 {
    4 + layer_count * LAYER_DEF_ENTRY_SIZE  // LayerCount(u32) + entries
}

pub(super) fn sub_layer_def_body_length(layer_count: u32) -> u32 {
    // SubLayerDef table also has LayerCount(u32) + Index(u32) before the entries
    4 + 4 + layer_count * SUB_LAYER_DEF_ENTRY_SIZE
}

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

pub(super) fn write_layer_def_body(
    out: &mut Cursor<Vec<u8>>,
    layers: &[AffLayerEntry],
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

pub(super) fn write_sub_layer_def_body(
    out: &mut Cursor<Vec<u8>>,
    layers: &[AffLayerEntry],
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

pub(super) const FILEMARK_LEN_V1: u32 = 36;
pub(super) const FILEMARK_LEN_V515: u32 = 40;
pub(super) const FILEMARK_LEN_V516: u32 = 48;
pub(super) const FILEMARK_LEN_V517: u32 = 52;
pub(super) const FILEMARK_LEN_V518: u32 = 60;

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

pub(super) fn write_filemark(
    out: &mut Cursor<Vec<u8>>,
    version: u16,
    addresses: &FileMarkAddresses,
) -> std::io::Result<()> {
    let start = out.position();
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
    let written = out.position() - start;
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
        write_layer_def_body(&mut c, std::slice::from_ref(&entry)).unwrap();
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
        write_sub_layer_def_body(&mut c, std::slice::from_ref(&entry)).unwrap();
        // 4 (LayerCount) + 4 (Index) + 44 (entry: 3*u32 + 8*f32)
        assert_eq!(c.into_inner().len(), 52);
    }

    #[test]
    fn filemark_v1_is_36_bytes_with_ANYCUBIC_mark() {
        let mut c = Cursor::new(Vec::new());
        write_filemark(&mut c, 1, &FileMarkAddresses::default()).unwrap();
        let bytes = c.into_inner();
        assert_eq!(bytes.len(), 36);
        assert_eq!(&bytes[..8], b"ANYCUBIC");
        assert_eq!(&bytes[12..16], &1u32.to_le_bytes()); // Version
        assert_eq!(&bytes[16..20], &4u32.to_le_bytes()); // NumberOfTables
    }

    #[test]
    fn filemark_v518_is_60_bytes_with_11_tables() {
        let mut c = Cursor::new(Vec::new());
        write_filemark(&mut c, 518, &FileMarkAddresses::default()).unwrap();
        let bytes = c.into_inner();
        assert_eq!(bytes.len(), 60);
        assert_eq!(&bytes[16..20], &11u32.to_le_bytes());
    }
}
