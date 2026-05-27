//! Per-printer machine profiles for AFF (Anycubic File Format).
//!
//! Each of the 21 supported AFF extensions maps to a `AffMachineProfile`
//! const carrying the printer's identity (machine name, display size, etc.),
//! the highest format version supported, and which RLE codec is used.
//! Values are sourced from UVTools `AnycubicFile.cs` switch statements.

use super::aff_codec::AffRleFormat;
use crate::types::SliceJobV3;
use serde_json::Value;

#[derive(Debug, Clone)]
pub(super) struct AffMachineProfile {
    pub key_suffix: &'static str,
    pub machine_name: &'static str,
    pub max_version: u16,
    pub rle_format: AffRleFormat,
    pub display_width_mm: f32,
    pub display_height_mm: f32,
    pub machine_z_mm: f32,
    pub default_pixel_um: f32,
    pub layer_image_format: &'static str,
    pub preview_size: [u32; 2],
    pub preview2_size: [u32; 2],
}

// ── Per-extension profiles ──────────────────────────────────────────
// Values from UVTools AnycubicFile.cs DisplayWidth/DisplayHeight/MachineZ/MachineName
// switches and GetAvailableVersionsForExtension version table.

const PROFILE_PWS: AffMachineProfile = AffMachineProfile {
    key_suffix: "pws", machine_name: "Photon S", max_version: 1,
    rle_format: AffRleFormat::Pws,
    display_width_mm: 68.04, display_height_mm: 120.96, machine_z_mm: 165.0,
    default_pixel_um: 47.25, layer_image_format: "pwsImg",
    preview_size: [224, 168], preview2_size: [330, 190],
};

const PROFILE_PW0: AffMachineProfile = AffMachineProfile {
    key_suffix: "pw0", machine_name: "Photon Zero", max_version: 1,
    rle_format: AffRleFormat::Pw0,
    display_width_mm: 55.44, display_height_mm: 98.637, machine_z_mm: 150.0,
    default_pixel_um: 47.25, layer_image_format: "pw0Img",
    preview_size: [224, 168], preview2_size: [330, 190],
};

const PROFILE_PWX: AffMachineProfile = AffMachineProfile {
    key_suffix: "pwx", machine_name: "Photon X", max_version: 1,
    rle_format: AffRleFormat::Pw0,
    display_width_mm: 192.0, display_height_mm: 120.0, machine_z_mm: 245.0,
    default_pixel_um: 47.25, layer_image_format: "pw0Img",
    preview_size: [224, 168], preview2_size: [330, 190],
};

const PROFILE_DLP: AffMachineProfile = AffMachineProfile {
    key_suffix: "dlp", machine_name: "Photon Ultra", max_version: 516,
    rle_format: AffRleFormat::Pw0,
    display_width_mm: 102.40, display_height_mm: 57.60, machine_z_mm: 165.0,
    default_pixel_um: 47.25, layer_image_format: "pw0Img",
    preview_size: [224, 168], preview2_size: [330, 190],
};

const PROFILE_DL2P: AffMachineProfile = AffMachineProfile {
    key_suffix: "dl2p", machine_name: "Photon D2", max_version: 517,
    rle_format: AffRleFormat::Pw0,
    display_width_mm: 130.56, display_height_mm: 73.44, machine_z_mm: 165.0,
    default_pixel_um: 47.25, layer_image_format: "pw0Img",
    preview_size: [224, 168], preview2_size: [330, 190],
};

const PROFILE_PWMO: AffMachineProfile = AffMachineProfile {
    key_suffix: "pwmo", machine_name: "Photon Mono", max_version: 516,
    rle_format: AffRleFormat::Pw0,
    display_width_mm: 82.62, display_height_mm: 130.56, machine_z_mm: 165.0,
    default_pixel_um: 47.25, layer_image_format: "pw0Img",
    preview_size: [224, 168], preview2_size: [330, 190],
};

const PROFILE_PM3N: AffMachineProfile = AffMachineProfile {
    key_suffix: "pm3n", machine_name: "Photon Mono 2", max_version: 517,
    rle_format: AffRleFormat::Pw0,
    display_width_mm: 143.36, display_height_mm: 89.60, machine_z_mm: 165.0,
    default_pixel_um: 47.25, layer_image_format: "pw0Img",
    preview_size: [224, 168], preview2_size: [330, 190],
};

// NOTE: pm4n max_version=518 inferred from UVTools default fallthrough.
// Downgrade to 517 if firmware reports indicate v518 isn't accepted.
const PROFILE_PM4N: AffMachineProfile = AffMachineProfile {
    key_suffix: "pm4n", machine_name: "Photon Mono 4", max_version: 518,
    rle_format: AffRleFormat::Pw0,
    display_width_mm: 153.408, display_height_mm: 87.040, machine_z_mm: 165.0,
    default_pixel_um: 47.25, layer_image_format: "pw0Img",
    preview_size: [224, 168], preview2_size: [330, 190],
};

const PROFILE_PWMS: AffMachineProfile = AffMachineProfile {
    key_suffix: "pwms", machine_name: "Photon Mono SE", max_version: 516,
    rle_format: AffRleFormat::Pw0,
    display_width_mm: 82.62, display_height_mm: 130.56, machine_z_mm: 160.0,
    default_pixel_um: 47.25, layer_image_format: "pw0Img",
    preview_size: [224, 168], preview2_size: [330, 190],
};

const PROFILE_PWMA: AffMachineProfile = AffMachineProfile {
    key_suffix: "pwma", machine_name: "Photon Mono 4K", max_version: 516,
    rle_format: AffRleFormat::Pw0,
    display_width_mm: 134.40, display_height_mm: 84.0, machine_z_mm: 165.0,
    default_pixel_um: 47.25, layer_image_format: "pw0Img",
    preview_size: [224, 168], preview2_size: [330, 190],
};

const PROFILE_PWMX: AffMachineProfile = AffMachineProfile {
    key_suffix: "pwmx", machine_name: "Photon Mono X", max_version: 516,
    rle_format: AffRleFormat::Pw0,
    display_width_mm: 192.0, display_height_mm: 120.0, machine_z_mm: 245.0,
    default_pixel_um: 47.25, layer_image_format: "pw0Img",
    preview_size: [224, 168], preview2_size: [330, 190],
};

const PROFILE_PMX2: AffMachineProfile = AffMachineProfile {
    key_suffix: "pmx2", machine_name: "Photon Mono X2", max_version: 517,
    rle_format: AffRleFormat::Pw0,
    display_width_mm: 196.61, display_height_mm: 122.88, machine_z_mm: 200.0,
    default_pixel_um: 47.25, layer_image_format: "pw0Img",
    preview_size: [224, 168], preview2_size: [330, 190],
};

const PROFILE_PWMB: AffMachineProfile = AffMachineProfile {
    key_suffix: "pwmb", machine_name: "Photon Mono X 6K / M3 Plus", max_version: 517,
    rle_format: AffRleFormat::Pw0,
    display_width_mm: 198.15, display_height_mm: 123.84, machine_z_mm: 245.0,
    default_pixel_um: 47.25, layer_image_format: "pw0Img",
    preview_size: [224, 168], preview2_size: [330, 190],
};

const PROFILE_PX6S: AffMachineProfile = AffMachineProfile {
    key_suffix: "px6s", machine_name: "Photon Mono X 6Ks", max_version: 517,
    rle_format: AffRleFormat::Pw0,
    display_width_mm: 195.84, display_height_mm: 122.40, machine_z_mm: 200.0,
    default_pixel_um: 47.25, layer_image_format: "pw0Img",
    preview_size: [224, 168], preview2_size: [330, 190],
};

const PROFILE_PMSQ: AffMachineProfile = AffMachineProfile {
    key_suffix: "pmsq", machine_name: "Photon Mono SQ", max_version: 516,
    rle_format: AffRleFormat::Pw0,
    display_width_mm: 120.0, display_height_mm: 128.0, machine_z_mm: 200.0,
    default_pixel_um: 47.25, layer_image_format: "pw0Img",
    preview_size: [224, 168], preview2_size: [330, 190],
};

const PROFILE_PM3: AffMachineProfile = AffMachineProfile {
    key_suffix: "pm3", machine_name: "Photon M3", max_version: 516,
    rle_format: AffRleFormat::Pw0,
    display_width_mm: 163.84, display_height_mm: 102.40, machine_z_mm: 180.0,
    default_pixel_um: 47.25, layer_image_format: "pw0Img",
    preview_size: [224, 168], preview2_size: [330, 190],
};

const PROFILE_PM3M: AffMachineProfile = AffMachineProfile {
    key_suffix: "pm3m", machine_name: "Photon M3 Max", max_version: 516,
    rle_format: AffRleFormat::Pw0,
    display_width_mm: 298.08, display_height_mm: 165.60, machine_z_mm: 300.0,
    default_pixel_um: 47.25, layer_image_format: "pw0Img",
    preview_size: [224, 168], preview2_size: [330, 190],
};

const PROFILE_PM3R: AffMachineProfile = AffMachineProfile {
    key_suffix: "pm3r", machine_name: "Photon M3 Premium", max_version: 517,
    rle_format: AffRleFormat::Pw0,
    display_width_mm: 218.88, display_height_mm: 123.12, machine_z_mm: 250.0,
    default_pixel_um: 47.25, layer_image_format: "pw0Img",
    preview_size: [224, 168], preview2_size: [330, 190],
};

const PROFILE_PM5: AffMachineProfile = AffMachineProfile {
    key_suffix: "pm5", machine_name: "Photon Mono M5", max_version: 517,
    rle_format: AffRleFormat::Pw0,
    display_width_mm: 218.88, display_height_mm: 122.88, machine_z_mm: 200.0,
    default_pixel_um: 47.25, layer_image_format: "pw0Img",
    preview_size: [224, 168], preview2_size: [330, 190],
};

const PROFILE_PM5S: AffMachineProfile = AffMachineProfile {
    key_suffix: "pm5s", machine_name: "Photon Mono M5s", max_version: 518,
    rle_format: AffRleFormat::Pw0,
    display_width_mm: 218.88, display_height_mm: 122.88, machine_z_mm: 200.0,
    default_pixel_um: 47.25, layer_image_format: "pw0Img",
    preview_size: [224, 168], preview2_size: [330, 190],
};

const PROFILE_M5SP: AffMachineProfile = AffMachineProfile {
    key_suffix: "m5sp", machine_name: "Photon Mono M5s Pro", max_version: 518,
    rle_format: AffRleFormat::Pw0,
    display_width_mm: 223.6416, display_height_mm: 126.976, machine_z_mm: 200.0,
    default_pixel_um: 47.25, layer_image_format: "pw0Img",
    preview_size: [224, 168], preview2_size: [330, 190],
};

pub(super) fn machine_profile_for_suffix(suffix: &str) -> &'static AffMachineProfile {
    match suffix.to_ascii_lowercase().as_str() {
        "pws"  => &PROFILE_PWS,
        "pw0"  => &PROFILE_PW0,
        "pwx"  => &PROFILE_PWX,
        "dlp"  => &PROFILE_DLP,
        "dl2p" => &PROFILE_DL2P,
        "pwmo" => &PROFILE_PWMO,
        "pm3n" => &PROFILE_PM3N,
        "pm4n" => &PROFILE_PM4N,
        "pwms" => &PROFILE_PWMS,
        "pwma" => &PROFILE_PWMA,
        "pwmx" => &PROFILE_PWMX,
        "pmx2" => &PROFILE_PMX2,
        "pwmb" => &PROFILE_PWMB,
        "px6s" => &PROFILE_PX6S,
        "pmsq" => &PROFILE_PMSQ,
        "pm3"  => &PROFILE_PM3,
        "pm3m" => &PROFILE_PM3M,
        "pm3r" => &PROFILE_PM3R,
        "pm5"  => &PROFILE_PM5,
        "pm5s" => &PROFILE_PM5S,
        "m5sp" => &PROFILE_M5SP,
        other  => {
            eprintln!("[AFF] unknown keySuffix {other:?}, defaulting to Mono profile");
            &PROFILE_PWMO
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_aff_extension_resolves_to_distinct_profile() {
        let suffixes = [
            "pws","pw0","pwx","dlp","dl2p","pwmo","pm3n","pm4n","pwms","pwma",
            "pwmx","pmx2","pwmb","px6s","pmsq","pm3","pm3m","pm3r","pm5","pm5s","m5sp",
        ];
        for s in suffixes {
            let p = machine_profile_for_suffix(s);
            assert_eq!(p.key_suffix, s, "suffix {s} -> profile.key_suffix mismatch");
        }
    }

    #[test]
    fn unknown_suffix_falls_back_to_mono() {
        let p = machine_profile_for_suffix("nope");
        assert_eq!(p.key_suffix, "pwmo");
    }

    #[test]
    fn pws_uses_pws_rle_format() {
        assert!(matches!(machine_profile_for_suffix("pws").rle_format, AffRleFormat::Pws));
    }

    #[test]
    fn pw0_extensions_use_pw0_rle_format() {
        for s in ["pw0", "pm3m", "pm5s", "m5sp", "dlp"] {
            assert!(matches!(machine_profile_for_suffix(s).rle_format, AffRleFormat::Pw0),
                "{s} should use PW0");
        }
    }
}
