//! Bounded byte paging and NDJSON decoding. Raw capture is never rewritten.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs::File,
    io::{BufRead, Read, Seek, SeekFrom},
    path::Path,
};

pub const MAX_PAGE_BYTES: usize = 65536;
pub const MAX_RECORD_BYTES: usize = 65536;
const MAX_GAPS: usize = 32;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DecodeProvenance {
    pub scanned_bytes: u64,
    pub gaps: Vec<Gap>,
    pub omitted_gaps: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gap {
    pub start: u64,
    pub end: u64,
    pub reason: String,
}
pub struct Record {
    pub start: u64,
    pub end: u64,
    pub bytes: Option<Vec<u8>>,
}
pub struct Records<R> {
    reader: R,
    provenance: DecodeProvenance,
}
impl<R: BufRead> Records<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            provenance: DecodeProvenance::default(),
        }
    }
    pub fn gap(&mut self, start: u64, end: u64, reason: &str) {
        if self.provenance.gaps.len() < MAX_GAPS {
            self.provenance.gaps.push(Gap {
                start,
                end,
                reason: reason.into(),
            });
        } else {
            self.provenance.omitted_gaps += 1;
        }
    }
    pub fn provenance(&self) -> DecodeProvenance {
        self.provenance.clone()
    }
    pub fn next_record(&mut self) -> Result<Option<Record>> {
        let start = self.provenance.scanned_bytes;
        let mut bytes = Vec::new();
        let mut oversized = false;
        let complete = loop {
            let buffer = self.reader.fill_buf()?;
            if buffer.is_empty() {
                break false;
            }
            let newline = buffer.iter().position(|b| *b == b'\n');
            let take = newline.map_or(buffer.len(), |n| n + 1);
            if !oversized && bytes.len() + take <= MAX_RECORD_BYTES {
                bytes.extend_from_slice(&buffer[..take]);
            } else {
                bytes.clear();
                oversized = true;
            }
            self.reader.consume(take);
            self.provenance.scanned_bytes += take as u64;
            if newline.is_some() {
                break true;
            }
        };
        let end = self.provenance.scanned_bytes;
        if end == start {
            return Ok(None);
        }
        if oversized || !complete {
            self.gap(
                start,
                end,
                if oversized {
                    "oversized_record"
                } else {
                    "incomplete_record"
                },
            );
            return Ok(Some(Record {
                start,
                end,
                bytes: None,
            }));
        }
        Ok(Some(Record {
            start,
            end,
            bytes: Some(bytes),
        }))
    }
}

/// JSON metadata is also bounded; I/O/parse failures remain errors, never empty state.
pub fn json_file(path: &Path) -> Result<Value> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(256 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 256 * 1024,
        "retained metadata exceeds decoding limit"
    );
    Ok(serde_json::from_slice(&bytes)?)
}

/// Byte cursors are lossless even across UTF-8 boundaries. Text is only a preview.
pub fn page(path: &Path, after: u64, limit: usize) -> Result<Value> {
    ensure!(
        (1..=MAX_PAGE_BYTES).contains(&limit),
        "output limit must be 1..65536 bytes"
    );
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            ensure!(after == 0, "output cursor is beyond unavailable capture");
            return Ok(
                json!({"capture_availability":"unavailable","capture_state":"unknown","offset":0,"next_offset":0,"captured_bytes":null,"bytes":[],"text":null,"has_more":false,"gap":{"reason":"raw_capture_missing","start":0,"end":null}}),
            );
        }
        Err(error) => return Err(error.into()),
    };
    let captured = file.metadata()?.len();
    ensure!(after <= captured, "output cursor is beyond captured bytes");
    file.seek(SeekFrom::Start(after))?;
    let mut bytes = Vec::new();
    file.take((limit as u64).min(captured - after))
        .read_to_end(&mut bytes)?;
    let next = after + bytes.len() as u64;
    let text = String::from_utf8_lossy(&bytes);
    Ok(
        json!({"capture_availability":"available","capture_state":"retained_bytes_only","offset":after,"next_offset":next,"captured_bytes":captured,"bytes":bytes,"text":text,"text_encoding":"utf8_lossy_preview; bytes and offsets authoritative","has_more":next<captured,"gap":if next<captured && next==after {json!({"reason":"capture_changed_during_read","start":next,"end":captured})} else {Value::Null}}),
    )
}
