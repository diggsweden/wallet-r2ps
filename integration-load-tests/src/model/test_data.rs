// SPDX-FileCopyrightText: 2026 Digg - Agency for Digital Government
//
// SPDX-License-Identifier: EUPL-1.2

//! Test data model and gzip JSON I/O.

use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read};
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TestDataEnvelope {
    pub opaque_context: String,
    pub opaque_server_identifier: String,
    pub clients: Vec<ClientTestData>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ClientTestData {
    pub client_id: String,
    pub kid: String,
    pub pin: String,
    pub pin_stretch_d: String,
    pub device_key: DeviceKey,
    pub hsm_kid: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DeviceKey {
    pub kty: String,
    pub crv: String,
    pub x: String,
    pub y: String,
    pub d: String,
    pub kid: String,
}

impl TestDataEnvelope {
    /// Write the test data as gzip-compressed JSON.
    pub fn write_gzip(&self, path: &Path) -> Result<()> {
        let file = File::create(path)
            .with_context(|| format!("Failed to create file: {}", path.display()))?;
        let writer = BufWriter::new(file);
        let mut encoder = GzEncoder::new(writer, Compression::default());
        serde_json::to_writer_pretty(&mut encoder, self)
            .context("Failed to serialize test data")?;
        encoder.finish().context("Failed to finish gzip")?;
        Ok(())
    }

    /// Read test data from a file. Auto-detects gzip vs plain JSON by extension.
    pub fn read_from(path: &Path) -> Result<Self> {
        let file =
            File::open(path).with_context(|| format!("Failed to open file: {}", path.display()))?;
        let reader = BufReader::new(file);

        let extension = path.to_str().unwrap_or("");

        if extension.ends_with(".gz") {
            let mut decoder = GzDecoder::new(reader);
            let mut json_str = String::new();
            decoder
                .read_to_string(&mut json_str)
                .context("Failed to decompress gzip")?;
            serde_json::from_str(&json_str).context("Failed to parse test data JSON")
        } else {
            serde_json::from_reader(reader).context("Failed to parse test data JSON")
        }
    }
}
