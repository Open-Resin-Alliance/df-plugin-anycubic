use crate::engine::SlicerV3Error;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_SPOOL: AtomicU64 = AtomicU64::new(0);

/// One layer's payload extent; payload bytes are never retained between layers.
pub(super) struct StoredLayer {
    pub index: usize,
    pub offset: u64,
    pub encoded_len: u64,
    pub non_zero_pixel_count: u32,
}

/// Append-only payload storage. Physical order may differ from layer order.
pub(super) struct LayerSpool {
    file: Option<File>,
    path: PathBuf,
    layers: Vec<StoredLayer>,
    offset: u64,
    error: Option<io::Error>,
}

impl LayerSpool {
    pub fn new(layer_count: u32) -> Result<Self, SlicerV3Error> {
        loop {
            let sequence = NEXT_SPOOL.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "dragonfruit-anycubic-{}-{sequence}.layers",
                std::process::id(),
            ));
            let mut options = OpenOptions::new();
            options.read(true).write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(file) => {
                    return Ok(Self {
                        file: Some(file),
                        path,
                        layers: Vec::with_capacity(layer_count as usize),
                        offset: 0,
                        error: None,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
    }

    /// The parallel encoder callback cannot return an error. Remember the first
    /// write failure and report it from consume/finalize rather than losing it.
    pub fn store(&mut self, index: usize, bytes: Vec<u8>, non_zero_pixel_count: u32) {
        if self.error.is_some() {
            return;
        }
        match self.file.as_mut().expect("spool is open until drop").write_all(&bytes) {
            Ok(()) => {
                self.layers.push(StoredLayer {
                    index,
                    offset: self.offset,
                    encoded_len: bytes.len() as u64,
                    non_zero_pixel_count,
                });
                self.offset += bytes.len() as u64;
            }
            Err(error) => self.error = Some(error),
        }
    }

    pub fn check(&self) -> Result<(), SlicerV3Error> {
        match self.error.as_ref() {
            Some(error) => Err(SlicerV3Error::Zip(format!("Anycubic layer spool write failed: {error}"))),
            None => Ok(()),
        }
    }

    pub fn ordered_parts(&mut self) -> Result<(&[StoredLayer], &mut File), SlicerV3Error> {
        self.check()?;
        if self.layers.is_empty() {
            return Err(SlicerV3Error::MissingRenderedLayerPayload(
                "no rendered layers were provided for Anycubic encoding".to_string(),
            ));
        }
        self.layers.sort_unstable_by_key(|layer| layer.index);
        for (index, layer) in self.layers.iter().enumerate() {
            if layer.index != index {
                return Err(SlicerV3Error::MissingRenderedLayerPayload(format!(
                    "Anycubic layer {index} missing or duplicated in encoded output",
                )));
            }
        }
        Ok((&self.layers, self.file.as_mut().expect("spool is open until drop")))
    }

    pub fn layers_mut(&mut self) -> &mut [StoredLayer] {
        &mut self.layers
    }
}

impl Drop for LayerSpool {
    fn drop(&mut self) {
        // Windows cannot remove an open file: close before unlinking it.
        drop(self.file.take());
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_failure_survives_the_infallible_encoded_layer_callback() {
        let mut spool = LayerSpool::new(1).unwrap();
        spool.file = Some(File::open(&spool.path).unwrap());
        spool.store(0, vec![1, 2, 3], 0);
        assert!(spool.ordered_parts().is_err());
        assert!(spool.layers.is_empty());
    }

    #[test]
    fn dropping_an_unfinished_job_removes_its_payload_file() {
        let path = {
            let mut spool = LayerSpool::new(1).unwrap();
            spool.store(0, vec![1, 2, 3], 0);
            spool.path.clone()
        };
        assert_eq!(std::fs::metadata(path).unwrap_err().kind(), io::ErrorKind::NotFound);
    }
}
