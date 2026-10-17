//! Fixed binary layout. This parser is a bounds-checked, filesystem-free fuzz target.
use crate::{Error, Result};

pub const FRAME_SIZE: usize = 4096;
pub const BLOCK_SIZE: usize = 4028;
pub const MAX_FILE_SIZE: u64 = 1 << 30;
pub const MAX_BLOCKS: u32 = MAX_FILE_SIZE.div_ceil(BLOCK_SIZE as u64) as u32;
pub const META_INDEX: u32 = u32::MAX;
/// Versioned predefined geometry; dimensions are never taken from untrusted metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Profile {
    pub id: u8,
    pub matrix_size: usize,
}
impl Profile {
    pub fn frame_size(self) -> usize {
        self.matrix_size * self.matrix_size / 4
    }
    pub fn block_size(self) -> usize {
        self.frame_size() - 68
    }
}
pub const PROFILES: [Profile; 3] = [
    Profile {
        id: 1,
        matrix_size: 128,
    },
    Profile {
        id: 2,
        matrix_size: 160,
    },
    Profile {
        id: 3,
        matrix_size: 256,
    },
];
pub type TransferId = [u8; 16];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameType {
    Metadata,
    Data,
}

/// Validated fixed header; metadata's index is META_INDEX, never a storage offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameHeader {
    /// CRC-covered predefined geometry, locked for this transfer.
    pub profile: Profile,
    /// Metadata initializes the session; data is stored only after initialization.
    pub kind: FrameType,
    /// Stable across all repetitions of one prepared file.
    pub transfer_id: TransferId,
    /// Zero-based data index, or META_INDEX for metadata.
    pub block_index: u32,
    /// Number of unique data blocks, excluding all metadata repetitions.
    pub total_blocks: u32,
    /// Effective bytes; excludes the zero padding and CRC trailer.
    pub payload_len: u16,
}

/// Metadata is immutable once accepted. Filename is still untrusted filesystem input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    /// Exact uncompressed input length, bounded by MAX_FILE_SIZE at parse time.
    pub file_size: u64,
    /// Profile-specific fixed payload capacity, including short-final-block transfers.
    pub block_size: u32,
    /// Digest of original file bytes in index order, not of image or padded frames.
    pub sha256: [u8; 32],
    /// Strict UTF-8, not normalized; safe_basename is mandatory before publication.
    pub filename: String,
}
impl Metadata {
    /// Exact block length; call only after checking index against total_blocks().
    pub fn block_len(&self, index: u32) -> Option<usize> {
        if index >= self.total_blocks() {
            return None;
        }
        Some(
            (self.file_size - index as u64 * self.block_size as u64).min(self.block_size as u64)
                as usize,
        )
    }
    pub fn total_blocks(&self) -> u32 {
        self.file_size.div_ceil(self.block_size as u64) as u32
    }
}

/// Only parse_frame can construct this value. Consumers cannot bypass frame CRC.
#[derive(Debug, Clone)]
pub struct ValidatedFrame {
    header: FrameHeader,
    payload: Vec<u8>,
    metadata: Option<Metadata>,
}
impl ValidatedFrame {
    pub fn header(&self) -> &FrameHeader {
        &self.header
    }
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
    pub fn metadata(&self) -> Option<&Metadata> {
        self.metadata.as_ref()
    }
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}
fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
}

/// Verify all frame-local invariants before returning an owned, bounded frame.
/// The final block's exact length is checked later against the session metadata.
pub fn parse_frame(bytes: &[u8]) -> Result<ValidatedFrame> {
    let profile = PROFILES
        .iter()
        .find(|p| p.frame_size() == bytes.len())
        .copied()
        .ok_or(Error::Invalid("frame size"))?;
    let end = bytes.len() - 4;
    let block_size = profile.block_size();
    if crc32fast::hash(&bytes[..end]) != u32_at(bytes, end) {
        return Err(Error::Crc);
    }
    if &bytes[..4] != b"VMT1"
        || bytes[4] != 1
        || bytes[6] != profile.id
        || bytes[7] != 0
        || bytes[34..64].iter().any(|&b| b != 0)
    {
        return Err(Error::Invalid("version/profile/flags/reserved"));
    }
    let kind = match bytes[5] {
        1 => FrameType::Metadata,
        2 => FrameType::Data,
        _ => return Err(Error::Invalid("frame type")),
    };
    let index = u32_at(bytes, 24);
    let total = u32_at(bytes, 28);
    let len = u16_at(bytes, 32) as usize;
    if len > block_size || total > MAX_FILE_SIZE.div_ceil(block_size as u64) as u32 {
        return Err(Error::Invalid("payload length/block count"));
    }
    if bytes[64 + len..end].iter().any(|&b| b != 0) {
        return Err(Error::Invalid("nonzero padding"));
    }
    let payload = &bytes[64..64 + len];
    let metadata = match kind {
        FrameType::Metadata => {
            if index != META_INDEX || len < 47 {
                return Err(Error::Invalid("metadata header"));
            }
            let file_size = u64::from_le_bytes(payload[..8].try_into().unwrap());
            let name_len = u16_at(payload, 44) as usize;
            if file_size > MAX_FILE_SIZE
                || u32_at(payload, 8) != block_size as u32
                || name_len == 0
                || name_len > 1024
                || len != 46 + name_len
            {
                return Err(Error::Invalid("metadata fields"));
            }
            let meta = Metadata {
                file_size,
                block_size: block_size as u32,
                sha256: payload[12..44].try_into().unwrap(),
                filename: std::str::from_utf8(&payload[46..])
                    .map_err(|_| Error::Invalid("filename UTF-8"))?
                    .to_owned(),
            };
            if meta.total_blocks() != total {
                return Err(Error::Invalid("metadata block count"));
            }
            Some(meta)
        }
        FrameType::Data => {
            if total == 0 || index >= total || len == 0 || (index + 1 < total && len != block_size)
            {
                return Err(Error::Invalid("data index/length"));
            }
            None
        }
    };
    Ok(ValidatedFrame {
        header: FrameHeader {
            profile,
            kind,
            transfer_id: bytes[8..24].try_into().unwrap(),
            block_index: index,
            total_blocks: total,
            payload_len: len as u16,
        },
        payload: payload.to_vec(),
        metadata,
    })
}

/// Inverse of the JS symbol packer; white (4) is rejected, never mapped to black.
pub fn unpack_symbols(symbols: &[u8]) -> Result<Vec<u8>> {
    if !PROFILES
        .iter()
        .any(|p| symbols.len() == p.matrix_size * p.matrix_size)
    {
        return Err(Error::Invalid("symbol count"));
    }
    let mut out = vec![0u8; symbols.len() / 4];
    for (i, &s) in symbols.iter().enumerate() {
        if s > 3 {
            return Err(Error::Invalid("data symbol"));
        }
        out[i / 4] |= s << (6 - (i % 4) * 2);
    }
    Ok(out)
}
