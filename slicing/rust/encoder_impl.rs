//! Anycubic plugin encoder implementations.
//!
//! Provides two encoders:
//! - `AffPluginEncoder` — Anycubic File Format (.aff), used by Photon and Photon Mono series
//! - `AzfPluginEncoder` — Anycubic Zip Format (.azf), used by Photon Mono M7 series and Mono 4 Ultra

mod aff_codec;
mod aff_layout;
mod aff_metadata;
mod aff_preview;
mod afz_layout;
mod afz_metadata;
mod afz_preview;
mod anycubic_preview_common;
mod anycubic_pw0;
mod layer_spool;

use crate::encoders::{FormatEncoder, RawMaskStreamEncoder, RleStreamEncoder};
use crate::engine::SlicerV3Error;
use crate::types::{LayerAreaStatsV3, RenderedLayersV3, SliceJobV3};

use afz_layout::{build_afz_container, write_afz_container};
use afz_metadata::{machine_profile_for_suffix, parse_afz_build_model, parse_afz_timing_model};

use crossbeam_channel::bounded;
use layer_spool::LayerSpool;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

pub struct AffPluginEncoder;
pub struct AzfPluginEncoder;

pub fn create_plugin_encoder() -> Vec<Box<dyn FormatEncoder>> {
    vec![
        Box::new(AffPluginEncoder),
        Box::new(AzfPluginEncoder),
    ]
}

// ═══════════════════════════════════════════════════════════════════
//  AFF FormatEncoder trait impl
// ═══════════════════════════════════════════════════════════════════

impl FormatEncoder for AffPluginEncoder {
    fn output_format(&self) -> &'static str {
        ".aff"
    }

    fn requires_raw_mask_layers(&self) -> bool {
        // Only the non-streaming fallback reads raw masks; both PW0 and PWS
        // stream RLE.
        true
    }

    fn requires_png_layers(&self) -> bool {
        false
    }

    fn requires_area_stats(&self) -> bool {
        true // needed for NonZeroPixelCount + volume calculation
    }

    fn create_rle_stream_encoder(
        &self,
        job: &SliceJobV3,
    ) -> Result<Option<Box<dyn RleStreamEncoder>>, SlicerV3Error> {
        let build = parse_aff_build_model(job);
        // PWS packs one bit plane per AA level, so its layers are encoded from
        // the expanded mask; PW0 encodes straight from the runs.
        let pws_aa_level = (aff_rle_format_for_suffix(&build.key_suffix) == AffRleFormat::Pws)
            .then(|| parse_aff_timing_model(job).anti_alias_level as u8);
        let total_pixels =
            (job.source_width_px as usize).saturating_mul(job.source_height_px as usize);
        Ok(Some(Box::new(AffRleStreamEncoder {
            job: job.clone(),
            total_pixels,
            pws_aa_level,
            spool: LayerSpool::new(job.total_layers)?,
        })))
    }

    fn encode_container_from_rendered_layers(
        &self,
        job: &SliceJobV3,
        rendered_layers: &RenderedLayersV3,
        layer_area_stats: &[LayerAreaStatsV3],
    ) -> Result<Vec<u8>, SlicerV3Error> {
        self.encode_container_from_rendered_layers_with_progress(
            job, rendered_layers, layer_area_stats, None,
        )
    }

    fn estimate_encode_progress_units(&self, rendered_layers: &RenderedLayersV3) -> u32 {
        rendered_layers.raw_mask_layers.as_ref().map(|l| l.len() as u32).unwrap_or(0).saturating_mul(2)
    }

    fn encode_container_from_rendered_layers_with_progress(
        &self,
        job: &SliceJobV3,
        rendered_layers: &RenderedLayersV3,
        _layer_area_stats: &[LayerAreaStatsV3],
        on_progress: Option<&dyn Fn(u32, u32)>,
    ) -> Result<Vec<u8>, SlicerV3Error> {
        let mut spool = prepare_aff_layers(job, rendered_layers, on_progress)?;
        let timing = parse_aff_timing_model(job);
        let build = parse_aff_build_model(job);
        let profile = aff_machine_profile_for_suffix(&build.key_suffix);
        let (layers, payloads) = spool.ordered_parts()?;
        let bytes = build_aff_container(job, &timing, &build, profile, layers, payloads)?;
        if let Some(progress) = on_progress {
            let total = job.total_layers.saturating_mul(2);
            progress(total, total);
        }
        Ok(bytes)
    }

    fn encode_container_to_path(
        &self, job: &SliceJobV3, rendered_layers: &RenderedLayersV3,
        layer_area_stats: &[LayerAreaStatsV3], output_path: &Path,
    ) -> Result<(), SlicerV3Error> {
        self.encode_container_to_path_with_progress(job, rendered_layers, layer_area_stats, output_path, None)
    }

    fn encode_container_to_path_with_progress(
        &self, job: &SliceJobV3, rendered_layers: &RenderedLayersV3,
        _layer_area_stats: &[LayerAreaStatsV3], output_path: &Path,
        on_progress: Option<&dyn Fn(u32, u32)>,
    ) -> Result<(), SlicerV3Error> {
        let mut spool = prepare_aff_layers(job, rendered_layers, on_progress)?;
        let timing = parse_aff_timing_model(job);
        let build = parse_aff_build_model(job);
        let profile = aff_machine_profile_for_suffix(&build.key_suffix);
        let (layers, payloads) = spool.ordered_parts()?;
        let mut output = write_aff_container(job, &timing, &build, profile, layers,
            payloads, BufWriter::new(File::create(output_path)?))?;
        output.flush()?;
        if let Some(progress) = on_progress {
            let total = job.total_layers.saturating_mul(2);
            progress(total, total);
        }
        Ok(())
    }
}

fn prepare_aff_layers(
    job: &SliceJobV3,
    rendered_layers: &RenderedLayersV3,
    on_progress: Option<&dyn Fn(u32, u32)>,
) -> Result<LayerSpool, SlicerV3Error> {
    let raw_masks = rendered_layers.raw_mask_layers.as_ref().filter(|layers| !layers.is_empty())
        .ok_or_else(|| SlicerV3Error::MissingRenderedLayerPayload("raw mask layers are required for AFF encoding".to_string()))?;
    let expected_pixels = job.source_width_px as usize * job.source_height_px as usize;
    let timing = parse_aff_timing_model(job);
    let build = parse_aff_build_model(job);
    let pws = aff_rle_format_for_suffix(&build.key_suffix) == AffRleFormat::Pws;
    let mut spool = LayerSpool::new(job.total_layers)?;
    for (index, mask) in raw_masks.iter().enumerate() {
        if mask.len() != expected_pixels {
            return Err(SlicerV3Error::MissingRenderedLayerPayload(format!(
                "AFF layer {index} size mismatch: expected {expected_pixels} bytes, got {}", mask.len())));
        }
        let non_zero = mask.iter().filter(|&&value| value > 0).count() as u32;
        let encoded = if pws { encode_pws(mask, timing.anti_alias_level as u8) }
            else { anycubic_pw0::encode_pw0(mask) };
        spool.store(index, encoded, non_zero);
        spool.check()?;
        if let Some(progress) = on_progress {
            progress((index as u32).saturating_mul(2).saturating_add(1), job.total_layers.saturating_mul(2));
        }
    }
    Ok(spool)
}

// ═══════════════════════════════════════════════════════════════════
//  AZF threading helpers
// ═══════════════════════════════════════════════════════════════════

fn choose_afz_encode_threads() -> usize {
    let hw = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let env = std::env::var("DF_V3_AFZ_ENCODE_THREADS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v >= 1)
        .unwrap_or(hw);
    env.clamp(1, hw)
}

fn cap_afz_workers_for_mask_bytes(requested: usize, bytes_per_mask: usize) -> usize {
    // Allow operator override for memory budget (DF_V3_AFZ_INFLIGHT_MB / DF_V3_AFF_INFLIGHT_MB).
    let env_mb = std::env::var("DF_V3_AFZ_INFLIGHT_MB")
        .ok()
        .or_else(|| std::env::var("DF_V3_AFF_INFLIGHT_MB").ok())
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0);

    if let Some(max_mb) = env_mb {
        let max_bytes = max_mb.saturating_mul(1024 * 1024);
        let max_workers = if bytes_per_mask > 0 {
            (max_bytes / bytes_per_mask).clamp(1, requested)
        } else {
            requested
        };
        return max_workers.max(1);
    }

    let mut capped = requested.max(1);

    if bytes_per_mask >= 48 * 1024 * 1024 {
        capped = capped.min(2);
    } else if bytes_per_mask >= 24 * 1024 * 1024 {
        capped = capped.min(4);
    } else if bytes_per_mask >= 12 * 1024 * 1024 {
        capped = capped.min(8);
    }

    capped.max(1)
}

fn choose_afz_queue_depth(worker_count: usize, bytes_per_mask: usize) -> usize {
    if bytes_per_mask >= 48 * 1024 * 1024 {
        1
    } else if bytes_per_mask >= 24 * 1024 * 1024 {
        2
    } else if bytes_per_mask >= 12 * 1024 * 1024 {
        3
    } else {
        (worker_count.saturating_mul(3)).clamp(3, 24)
    }
}

struct EncodedLayer {
    index: usize,
    encoded: Vec<u8>,
    non_zero_pixel_count: u32,
}

/// PW0-encode a single raw mask layer.
fn encode_single_afz_layer(index: usize, raw_mask: &[u8]) -> EncodedLayer {
    let non_zero = raw_mask.iter().filter(|&&b| b > 0).count() as u32;
    let encoded = anycubic_pw0::encode_pw0(raw_mask);
    EncodedLayer {
        index,
        encoded,
        non_zero_pixel_count: non_zero,
    }
}

/// PW0-encode an all-black empty layer without materializing a full-size zero mask.
/// Used when the streaming pipeline sends a 0-byte sentinel for layers with no geometry.
fn encode_single_afz_empty_layer(index: usize, pixel_count: usize) -> EncodedLayer {
    let encoded = anycubic_pw0::encode_pw0_from_rle(&[], pixel_count);
    EncodedLayer {
        index,
        encoded,
        non_zero_pixel_count: 0,
    }
}


// ═══════════════════════════════════════════════════════════════════
//  AZF streaming encoder (multi-threaded)
// ═══════════════════════════════════════════════════════════════════

struct AzfStreamEncoder {
    job: SliceJobV3,
    work_tx: Option<crossbeam_channel::Sender<(u32, Vec<u8>)>>,
    result_rx: mpsc::Receiver<Result<EncodedLayer, SlicerV3Error>>,
    workers: Vec<thread::JoinHandle<()>>,
    consumed_layers: u32,
    received_layers: u32,
    spool: LayerSpool,
}

struct AzfRleStreamEncoder {
    job: SliceJobV3,
    total_pixels: usize,
    spool: LayerSpool,
    area_stats: Vec<LayerAreaStatsV3>,
}

impl AzfStreamEncoder {
    fn store_result(&mut self, result: Result<EncodedLayer, SlicerV3Error>) -> Result<(), SlicerV3Error> {
        let layer = result?;
        self.spool.store(layer.index, layer.encoded, layer.non_zero_pixel_count);
        self.received_layers += 1;
        self.spool.check()
    }

    fn finish_workers(&mut self) -> Result<(), SlicerV3Error> {
        if self.consumed_layers == 0 {
            return Err(SlicerV3Error::MissingRenderedLayerPayload("no rendered layers were provided for AFZ encoding".to_string()));
        }
        self.work_tx.take();
        while let Some(worker) = self.workers.pop() {
            worker.join().map_err(|_| SlicerV3Error::UnsupportedOutput("AFZ streaming worker panicked".to_string()))?;
        }
        while self.received_layers < self.consumed_layers {
            let result = self.result_rx.recv().map_err(|_| SlicerV3Error::MissingRenderedLayerPayload("AFZ streaming worker results ended unexpectedly".to_string()))?;
            self.store_result(result)?;
        }
        Ok(())
    }
}

impl RawMaskStreamEncoder for AzfStreamEncoder {
    fn consume_raw_mask_layer(&mut self, layer_index: u32, raw_mask: Vec<u8>) -> Result<(), SlicerV3Error> {
        // Drain before accepting more work: only the bounded work queue and its
        // workers can accumulate results, never the entire print job.
        while let Ok(result) = self.result_rx.try_recv() {
            self.store_result(result)?;
        }
        let producer = self.work_tx.as_ref().ok_or_else(|| SlicerV3Error::MissingRenderedLayerPayload("AFZ streaming encoder no longer accepts layers after finalize".to_string()))?;
        producer.send((layer_index, raw_mask)).map_err(|_| SlicerV3Error::MissingRenderedLayerPayload("AFZ streaming worker channel closed unexpectedly".to_string()))?;
        self.consumed_layers += 1;
        Ok(())
    }

    fn finalize_to_bytes(mut self: Box<Self>) -> Result<Vec<u8>, SlicerV3Error> {
        self.finish_workers()?;
        let timing = parse_afz_timing_model(&self.job);
        let build = parse_afz_build_model(&self.job);
        let profile = machine_profile_for_suffix(&build.key_suffix);
        let (layers, payloads) = self.spool.ordered_parts()?;
        build_afz_container(&self.job, &timing, &build, profile, layers, &[], payloads, None)
    }

    fn finalize_to_path(mut self: Box<Self>, output_path: &Path) -> Result<(), SlicerV3Error> {
        self.finish_workers()?;
        let timing = parse_afz_timing_model(&self.job);
        let build = parse_afz_build_model(&self.job);
        let profile = machine_profile_for_suffix(&build.key_suffix);
        let (layers, payloads) = self.spool.ordered_parts()?;
        let mut output = write_afz_container(&self.job, &timing, &build, profile,
            layers, &[], payloads, BufWriter::new(File::create(output_path)?), None)?;
        output.flush()?;
        Ok(())
    }
}

impl RleStreamEncoder for AzfRleStreamEncoder {
    fn consume_rle_layer(&mut self, layer_index: u32, runs: Vec<crate::rle::RleRun>) -> Result<(), SlicerV3Error> {
        let encoded = anycubic_pw0::encode_pw0_from_rle(&runs, self.total_pixels);
        self.spool.store(layer_index as usize, encoded, 0);
        self.spool.check()
    }

    fn set_area_stats(&mut self, stats: Vec<LayerAreaStatsV3>) {
        self.area_stats = stats;
    }

    fn parallel_encode_fn(&self) -> Option<Arc<dyn Fn(u32, &[crate::rle::RleRun]) -> Result<Vec<u8>, SlicerV3Error> + Send + Sync>> {
        let total_pixels = self.total_pixels;
        Some(Arc::new(move |_index, runs| Ok(anycubic_pw0::encode_pw0_from_rle(runs, total_pixels))))
    }

    fn store_encoded_layer(&mut self, layer_index: u32, bytes: Vec<u8>) {
        self.spool.store(layer_index as usize, bytes, 0);
    }

    fn finalize_to_bytes(mut self: Box<Self>) -> Result<Vec<u8>, SlicerV3Error> {
        let timing = parse_afz_timing_model(&self.job);
        let build = parse_afz_build_model(&self.job);
        let profile = machine_profile_for_suffix(&build.key_suffix);
        let (layers, payloads) = self.spool.ordered_parts()?;
        build_afz_container(&self.job, &timing, &build, profile, layers, &self.area_stats, payloads, None)
    }

    fn finalize_to_path(mut self: Box<Self>, output_path: &Path) -> Result<(), SlicerV3Error> {
        let timing = parse_afz_timing_model(&self.job);
        let build = parse_afz_build_model(&self.job);
        let profile = machine_profile_for_suffix(&build.key_suffix);
        let (layers, payloads) = self.spool.ordered_parts()?;
        let mut output = write_afz_container(&self.job, &timing, &build, profile,
            layers, &self.area_stats, payloads, BufWriter::new(File::create(output_path)?), None)?;
        output.flush()?;
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════
//  AZF FormatEncoder trait impl
// ═══════════════════════════════════════════════════════════════════

impl FormatEncoder for AzfPluginEncoder {
    fn output_format(&self) -> &'static str {
        ".azf"
    }

    fn requires_raw_mask_layers(&self) -> bool {
        true
    }

    fn requires_png_layers(&self) -> bool {
        false
    }

    fn create_raw_mask_stream_encoder(
        &self,
        job: &SliceJobV3,
    ) -> Result<Option<Box<dyn RawMaskStreamEncoder>>, SlicerV3Error> {
        let expected_pixels =
            (job.source_width_px as usize).saturating_mul(job.source_height_px as usize);

        let worker_count =
            cap_afz_workers_for_mask_bytes(choose_afz_encode_threads(), expected_pixels);
        let queue_depth = choose_afz_queue_depth(worker_count, expected_pixels);
        let (work_tx, work_rx) = bounded::<(u32, Vec<u8>)>(queue_depth);
        let (result_tx, result_rx) = mpsc::channel::<Result<EncodedLayer, SlicerV3Error>>();
        let mut workers = Vec::with_capacity(worker_count);

        for _ in 0..worker_count {
            let work_rx = work_rx.clone();
            let result_tx = result_tx.clone();
            let worker_expected_pixels = expected_pixels;

            let handle = thread::spawn(move || loop {
                let Ok((layer_index, raw_mask)) = work_rx.recv() else {
                    break;
                };

                // Empty sentinel from pipeline = layer with no geometry (all black).
                if raw_mask.is_empty() {
                    let prepared = encode_single_afz_empty_layer(
                        layer_index as usize,
                        worker_expected_pixels,
                    );
                    crate::pipeline::return_mask_to_pool(raw_mask);
                    if result_tx.send(Ok(prepared)).is_err() {
                        break;
                    }
                    continue;
                }

                if raw_mask.len() != worker_expected_pixels {
                    let len = raw_mask.len();
                    crate::pipeline::return_mask_to_pool(raw_mask);
                    let _ = result_tx.send(Err(SlicerV3Error::MissingRenderedLayerPayload(
                        format!(
                            "AFZ layer {layer_index} size mismatch: expected {} bytes, got {}",
                            worker_expected_pixels, len
                        ),
                    )));
                    continue;
                }

                let prepared = encode_single_afz_layer(layer_index as usize, &raw_mask);
                crate::pipeline::return_mask_to_pool(raw_mask);

                if result_tx.send(Ok(prepared)).is_err() {
                    break;
                }
            });
            workers.push(handle);
        }

        // Drop extra clones so channels close properly when work_tx is dropped.
        drop(work_rx);
        drop(result_tx);

        Ok(Some(Box::new(AzfStreamEncoder {
            job: job.clone(),
            work_tx: Some(work_tx),
            result_rx,
            workers,
            consumed_layers: 0,
            received_layers: 0,
            spool: LayerSpool::new(job.total_layers)?,
        })))
    }

    fn create_rle_stream_encoder(
        &self,
        job: &SliceJobV3,
    ) -> Result<Option<Box<dyn RleStreamEncoder>>, SlicerV3Error> {
        let total_pixels =
            (job.source_width_px as usize).saturating_mul(job.source_height_px as usize);
        Ok(Some(Box::new(AzfRleStreamEncoder {
            job: job.clone(),
            total_pixels,
            spool: LayerSpool::new(job.total_layers)?,
            area_stats: vec![LayerAreaStatsV3::default(); job.total_layers as usize],
        })))
    }

    fn encode_container_from_rendered_layers(
        &self, job: &SliceJobV3, rendered_layers: &RenderedLayersV3,
        layer_area_stats: &[LayerAreaStatsV3],
    ) -> Result<Vec<u8>, SlicerV3Error> {
        self.encode_container_from_rendered_layers_with_progress(job, rendered_layers, layer_area_stats, None)
    }

    fn estimate_encode_progress_units(&self, rendered_layers: &RenderedLayersV3) -> u32 {
        rendered_layers.raw_mask_layers.as_ref().map(|l| l.len() as u32).unwrap_or(0).saturating_mul(2)
    }

    fn encode_container_from_rendered_layers_with_progress(
        &self, job: &SliceJobV3, rendered_layers: &RenderedLayersV3,
        layer_area_stats: &[LayerAreaStatsV3], on_progress: Option<&dyn Fn(u32, u32)>,
    ) -> Result<Vec<u8>, SlicerV3Error> {
        let mut spool = prepare_afz_layers(job, rendered_layers, on_progress)?;
        let timing = parse_afz_timing_model(job);
        let build = parse_afz_build_model(job);
        let profile = machine_profile_for_suffix(&build.key_suffix);
        let (layers, payloads) = spool.ordered_parts()?;
        let bytes = build_afz_container(job, &timing, &build, profile, layers, layer_area_stats, payloads, None)?;
        if let Some(progress) = on_progress {
            let total = job.total_layers.saturating_mul(2);
            progress(total, total);
        }
        Ok(bytes)
    }

    fn encode_container_to_path(
        &self, job: &SliceJobV3, rendered_layers: &RenderedLayersV3,
        layer_area_stats: &[LayerAreaStatsV3], output_path: &Path,
    ) -> Result<(), SlicerV3Error> {
        self.encode_container_to_path_with_progress(job, rendered_layers, layer_area_stats, output_path, None)
    }

    fn encode_container_to_path_with_progress(
        &self, job: &SliceJobV3, rendered_layers: &RenderedLayersV3,
        layer_area_stats: &[LayerAreaStatsV3], output_path: &Path,
        on_progress: Option<&dyn Fn(u32, u32)>,
    ) -> Result<(), SlicerV3Error> {
        let mut spool = prepare_afz_layers(job, rendered_layers, on_progress)?;
        let timing = parse_afz_timing_model(job);
        let build = parse_afz_build_model(job);
        let profile = machine_profile_for_suffix(&build.key_suffix);
        let (layers, payloads) = spool.ordered_parts()?;
        let mut output = write_afz_container(job, &timing, &build, profile, layers,
            layer_area_stats, payloads, BufWriter::new(File::create(output_path)?), None)?;
        output.flush()?;
        if let Some(progress) = on_progress {
            let total = job.total_layers.saturating_mul(2);
            progress(total, total);
        }
        Ok(())
    }

    fn read_layer_preview_png(
        &self,
        path: &Path,
        layer_number: u32,
    ) -> Result<Vec<u8>, SlicerV3Error> {
        self::read_layer_preview_png(path, layer_number).map_err(SlicerV3Error::LayerPreview)
    }
}

fn prepare_afz_layers(
    job: &SliceJobV3, rendered_layers: &RenderedLayersV3,
    on_progress: Option<&dyn Fn(u32, u32)>,
) -> Result<LayerSpool, SlicerV3Error> {
    let raw_masks = rendered_layers.raw_mask_layers.as_ref().filter(|layers| !layers.is_empty())
        .ok_or_else(|| SlicerV3Error::MissingRenderedLayerPayload("raw mask layers are required for AFZ encoding".to_string()))?;
    let expected_pixels = job.source_width_px as usize * job.source_height_px as usize;
    let mut spool = LayerSpool::new(job.total_layers)?;
    for (index, mask) in raw_masks.iter().enumerate() {
        if mask.len() != expected_pixels {
            return Err(SlicerV3Error::MissingRenderedLayerPayload(format!(
                "AFZ layer {index} size mismatch: expected {expected_pixels} bytes, got {}", mask.len())));
        }
        let layer = encode_single_afz_layer(index, mask);
        spool.store(index, layer.encoded, layer.non_zero_pixel_count);
        spool.check()?;
        if let Some(progress) = on_progress {
            progress((index as u32).saturating_mul(2).saturating_add(1), job.total_layers.saturating_mul(2));
        }
    }
    Ok(spool)
}

// ═══════════════════════════════════════════════════════════════════
//  AFF (Anycubic File Format) encoders
// ═══════════════════════════════════════════════════════════════════

use aff_codec::{aff_rle_format_for_suffix, encode_pw0_from_rle, encode_pws, AffRleFormat};
use aff_layout::{build_aff_container, write_aff_container};
use aff_metadata::{
    machine_profile_for_suffix as aff_machine_profile_for_suffix,
    parse_aff_build_model,
    parse_aff_timing_model,
};

struct AffRleStreamEncoder {
    job: SliceJobV3,
    total_pixels: usize,
    /// `Some(aa_level)` for PWS, `None` for PW0.
    pws_aa_level: Option<u8>,
    spool: LayerSpool,
}

impl RleStreamEncoder for AffRleStreamEncoder {
    fn consume_rle_layer(
        &mut self,
        layer_index: u32,
        runs: Vec<crate::rle::RleRun>,
    ) -> Result<(), SlicerV3Error> {
        let prepared = match self.pws_aa_level {
            Some(aa_level) => encode_single_aff_pws_layer(
                layer_index as usize,
                &crate::rle::expand_rle_to_mask(&runs, self.total_pixels),
                aa_level,
            ),
            None => EncodedLayer {
                index: layer_index as usize,
                encoded: encode_pw0_from_rle(&runs, self.total_pixels),
                non_zero_pixel_count: 0, // backfilled via set_area_stats
            },
        };
        self.spool.store(prepared.index, prepared.encoded, prepared.non_zero_pixel_count);
        self.spool.check()
    }

    fn set_area_stats(&mut self, stats: Vec<LayerAreaStatsV3>) {
        // PWS counts every lit pixel from the mask itself, as the raw-mask
        // path always did; only PW0 takes the rasterizer's solid count.
        if self.pws_aa_level.is_some() {
            return;
        }
        for layer in self.spool.layers_mut() {
            if let Some(s) = stats.get(layer.index) {
                layer.non_zero_pixel_count = s.total_solid_pixels;
            }
        }
    }

    fn parallel_encode_fn(
        &self,
    ) -> Option<Arc<dyn Fn(u32, &[crate::rle::RleRun]) -> Result<Vec<u8>, SlicerV3Error> + Send + Sync>> {
        // PWS needs each layer's lit-pixel count, which only consume_rle_layer
        // computes; it is a 2560×1440 panel, so the serial path is cheap.
        if self.pws_aa_level.is_some() {
            return None;
        }
        let total_pixels = self.total_pixels;
        Some(Arc::new(move |_idx, runs| Ok(encode_pw0_from_rle(runs, total_pixels))))
    }

    fn store_encoded_layer(&mut self, layer_index: u32, bytes: Vec<u8>) {
        self.spool.store(layer_index as usize, bytes, 0);
    }

    fn finalize_to_bytes(mut self: Box<Self>) -> Result<Vec<u8>, SlicerV3Error> {
        let timing = parse_aff_timing_model(&self.job);
        let build = parse_aff_build_model(&self.job);
        let profile = aff_machine_profile_for_suffix(&build.key_suffix);
        let (layers, payloads) = self.spool.ordered_parts()?;
        build_aff_container(&self.job, &timing, &build, profile, layers, payloads)
    }

    fn finalize_to_path(mut self: Box<Self>, output_path: &Path) -> Result<(), SlicerV3Error> {
        let timing = parse_aff_timing_model(&self.job);
        let build = parse_aff_build_model(&self.job);
        let profile = aff_machine_profile_for_suffix(&build.key_suffix);
        let (layers, payloads) = self.spool.ordered_parts()?;
        let mut output = write_aff_container(&self.job, &timing, &build, profile, layers,
            payloads, BufWriter::new(File::create(output_path)?))?;
        output.flush()?;
        Ok(())
    }
}

fn encode_single_aff_pws_layer(index: usize, raw_mask: &[u8], aa_level: u8) -> EncodedLayer {
    let non_zero = raw_mask.iter().filter(|&&b| b > 0).count() as u32;
    let encoded = encode_pws(raw_mask, aa_level);
    EncodedLayer { index, encoded, non_zero_pixel_count: non_zero }
}

/// Reads a single layer preview PNG from an AFZ (Anycubic Zip Format) file.
/// `layer_number` is 1-based.
pub fn read_layer_preview_png(path: &Path, layer_number: u32) -> Result<Vec<u8>, String> {
    use std::io::Read;

    if layer_number == 0 {
        return Err("Layer number must be >= 1".to_string());
    }

    let file = std::fs::File::open(path).map_err(|e| format!("Failed opening AFZ file: {e}"))?;
    let mut zip =
        zip::ZipArchive::new(file).map_err(|e| format!("Failed reading AFZ zip: {e}"))?;

    // Read settings JSON for canvas dimensions.
    let (width_px, height_px) = {
        let mut entry = zip
            .by_name("anycubic_photon_resins.pwsp")
            .map_err(|e| format!("AFZ settings file missing: {e}"))?;
        let mut json_str = String::new();
        entry
            .read_to_string(&mut json_str)
            .map_err(|e| format!("AFZ settings read failed: {e}"))?;
        let value: serde_json::Value = serde_json::from_str(&json_str)
            .map_err(|e| format!("AFZ settings JSON parse failed: {e}"))?;
        let machine = value
            .get("machine_type")
            .ok_or_else(|| "AFZ settings missing machine_type".to_string())?;
        let w = machine
            .get("res_x")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| "AFZ settings missing machine_type.res_x".to_string())?
            as u32;
        let h = machine
            .get("res_y")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| "AFZ settings missing machine_type.res_y".to_string())?
            as u32;
        (w, h)
    };

    if width_px == 0 || height_px == 0 {
        return Err(format!(
            "AFZ file reports invalid dimensions {width_px}×{height_px}"
        ));
    }

    let layer_index = layer_number - 1;
    let layer_name = format!("layer_images/layer_{}.pw0Img", layer_index);

    let mut rle_bytes = Vec::new();
    {
        let mut entry = zip.by_name(&layer_name).map_err(|e| {
            format!("AFZ layer {layer_number} not found ({layer_name}): {e}")
        })?;
        rle_bytes.reserve(entry.size() as usize);
        entry
            .read_to_end(&mut rle_bytes)
            .map_err(|e| format!("AFZ layer {layer_number} read failed: {e}"))?;
    }

    let expected_pixels = width_px as usize * height_px as usize;
    let pixels = anycubic_pw0::decode_pw0(&rle_bytes, expected_pixels);
    encode_pixels_as_grayscale_png(width_px, height_px, &pixels)
}

/// Encodes a flat 8-bit grayscale pixel buffer as a PNG.
fn encode_pixels_as_grayscale_png(
    width: u32,
    height: u32,
    pixels: &[u8],
) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| format!("AFZ PNG header write failed: {e}"))?;
    writer
        .write_image_data(pixels)
        .map_err(|e| format!("AFZ PNG data write failed: {e}"))?;
    drop(writer);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_job(output_format: &str, layers: u32) -> SliceJobV3 {
        SliceJobV3 {
            output_format: output_format.to_string(),
            format_version: None,
            source_width_px: 16,
            source_height_px: 16,
            width_px: 16,
            height_px: 16,
            build_width_mm: 10.0,
            build_depth_mm: 10.0,
            layer_height_mm: 0.05,
            total_layers: layers,
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


    /// Runs of a mask, as the rasterizer streams them.
    fn mask_to_runs(mask: &[u8]) -> Vec<crate::rle::RleRun> {
        let mut runs: Vec<crate::rle::RleRun> = Vec::new();
        for &value in mask {
            match runs.last_mut() {
                Some(run) if run.value == value => run.length += 1,
                _ => runs.push(crate::rle::RleRun { length: 1, value }),
            }
        }
        runs
    }

    #[test]
    fn pws_rle_stream_matches_the_raw_mask_encoding() {
        let mut job = make_job(".aff", 3);
        job.format_version = Some("pws".to_string());
        let pixels = 16 * 16;
        // Gray edges, an empty layer and a solid one: every case the AA planes see.
        let masks: Vec<Vec<u8>> = vec![
            (0..pixels).map(|i| ((i * 37) % 256) as u8).collect(),
            vec![0u8; pixels],
            vec![255u8; pixels],
        ];

        let mut stream = AffPluginEncoder.create_rle_stream_encoder(&job).unwrap().unwrap();
        for (index, mask) in masks.iter().enumerate() {
            stream.consume_rle_layer(index as u32, mask_to_runs(mask)).unwrap();
        }
        // The rasterizer's solid counts must not replace PWS's lit-pixel counts.
        stream.set_area_stats(vec![LayerAreaStatsV3::default(); masks.len()]);
        let streamed = stream.finalize_to_bytes().unwrap();

        let rendered = RenderedLayersV3 { raw_mask_layers: Some(masks), ..Default::default() };
        let from_masks = AffPluginEncoder
            .encode_container_from_rendered_layers(&job, &rendered, &[])
            .unwrap();

        assert_eq!(streamed, from_masks);
    }

    #[test]
    fn azf_parallel_results_preserve_layer_identity() {
        use std::io::Read;
        let job = make_job(".azf", 2);
        let mut stream = AzfPluginEncoder.create_rle_stream_encoder(&job).unwrap().unwrap();
        let encode = stream.parallel_encode_fn().unwrap();
        stream.store_encoded_layer(1, encode(1, &[crate::rle::RleRun { length: 256, value: 255 }]).unwrap());
        stream.store_encoded_layer(0, encode(0, &[]).unwrap());
        let bytes = stream.finalize_to_bytes().unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        for (index, value) in [(0, 0), (1, 255)] {
            let mut encoded = Vec::new();
            archive.by_name(&format!("layer_images/layer_{index}.pw0Img")).unwrap()
                .read_to_end(&mut encoded).unwrap();
            assert_eq!(anycubic_pw0::decode_pw0(&encoded, 256), vec![value; 256]);
        }
    }

    #[test]
    fn azf_raw_workers_preserve_layers_across_result_drains() {
        use std::io::Read;
        let job = make_job(".azf", 128);
        let mut stream = AzfPluginEncoder.create_raw_mask_stream_encoder(&job).unwrap().unwrap();
        for index in 0..128 {
            stream.consume_raw_mask_layer(index, vec![if index % 2 == 0 { 0 } else { 255 }; 256]).unwrap();
        }
        let bytes = stream.finalize_to_bytes().unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        for index in 0..128 {
            let mut encoded = Vec::new();
            archive.by_name(&format!("layer_images/layer_{index}.pw0Img")).unwrap()
                .read_to_end(&mut encoded).unwrap();
            assert_eq!(anycubic_pw0::decode_pw0(&encoded, 256), vec![if index % 2 == 0 { 0 } else { 255 }; 256]);
        }
    }


    #[test]
    fn encode_empty_layers_rejected() {
        let job = make_job(".aff", 1);
        let empty = RenderedLayersV3::default();
        let stats = vec![];
        let err = AffPluginEncoder
            .encode_container_from_rendered_layers(&job, &empty, &stats)
            .unwrap_err();
        let msg = format!("{err:?}").to_lowercase();
        assert!(msg.contains("raw mask layers") || msg.contains("no rendered layers"),
            "unexpected error: {msg}");
    }
}
