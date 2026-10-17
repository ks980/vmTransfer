//! Fixed binary layout. This parser is a bounds-checked, filesystem-free fuzz target.
use crate::{Error, Result};

pub const FRAME_SIZE: usize = 4096;
pub const BLOCK_SIZE: usize = 4028;
pub const MAX_FILE_SIZE: u64 = 1 << 30;
pub const MAX_BLOCKS: u32 = MAX_FILE_SIZE.div_ceil((128 * 128 / 8 - 68) as u64) as u32;
pub const META_INDEX: u32 = u32::MAX;
/// Versioned predefined geometry; dimensions are never taken from untrusted metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Profile {
    pub id: u8,
    pub matrix_width: usize,
    pub matrix_height: usize,
}
impl Profile {
    pub fn frame_size(self) -> usize {
        self.matrix_width * self.matrix_height / 4
    }
    pub fn block_size(self) -> usize {
        self.frame_size() - 68
    }
    pub fn frame_size_for(self, mode: u8) -> Option<usize> {
        let area = self.matrix_width * self.matrix_height;
        Some(match mode {
            2 => area / 8,
            4 => area / 4,
            8 => area * 3 / 8,
            16 => area / 2,
            24 => area * 3,
            _ => return None,
        })
    }
    pub fn block_size_for(self, mode: u8) -> Option<usize> {
        self.frame_size_for(mode).map(|n| n - 68)
    }
}
pub const PROFILES: [Profile; 7] = [
    Profile {
        id: 1,
        matrix_width: 128,
        matrix_height: 128,
    },
    Profile {
        id: 2,
        matrix_width: 160,
        matrix_height: 160,
    },
    Profile {
        id: 3,
        matrix_width: 256,
        matrix_height: 256,
    },
    Profile {
        id: 4,
        matrix_width: 512,
        matrix_height: 512,
    },
    Profile {
        id: 5,
        matrix_width: 1028,
        matrix_height: 512,
    },
    Profile {
        id: 6,
        matrix_width: 512,
        matrix_height: 256,
    },
    Profile {
        id: 7,
        matrix_width: 256,
        matrix_height: 128,
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
    /// 4 is the legacy VMT1 palette; 2/8/16/24 use the VMT2 bootstrap.
    pub color_mode: u8,
    /// Metadata initializes the session; data is stored only after initialization.
    pub kind: FrameType,
    /// Stable across all repetitions of one prepared file.
    pub transfer_id: TransferId,
    /// Zero-based data index, or META_INDEX for metadata.
    pub block_index: u32,
    /// Number of unique data blocks, excluding all metadata repetitions.
    pub total_blocks: u32,
    /// Effective bytes; excludes the zero padding and CRC trailer.
    pub payload_len: u32,
}

/// Metadata is immutable once accepted. Filename is still untrusted filesystem input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    /// Exact uncompressed input length, bounded by MAX_FILE_SIZE at parse time.
    pub file_size: u64,
    /// Profile-specific fixed payload capacity, including short-final-block transfers.
    pub block_size: u32,
    pub color_mode: u8,
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
    if bytes.len() < 68 {
        return Err(Error::Invalid("frame size"));
    }
    let end = bytes.len() - 4;
    if crc32fast::hash(&bytes[..end]) != u32_at(bytes, end) {
        return Err(Error::Crc);
    }
    let profile = PROFILES
        .iter()
        .find(|p| p.id == bytes[6])
        .copied()
        .ok_or(Error::Invalid("profile"))?;
    let mode = match bytes[4] {
        1 if bytes[7] == 0 => 4,
        2 if matches!(bytes[7], 2 | 8 | 16 | 24) => bytes[7],
        _ => return Err(Error::Invalid("version/color mode")),
    };
    if profile.frame_size_for(mode) != Some(bytes.len()) {
        return Err(Error::Invalid("frame size"));
    }
    let block_size = profile.block_size_for(mode).unwrap();
    if &bytes[..4] != b"VMT1"
        || bytes[6] != profile.id
        || bytes[if mode != 4 || profile.id == 5 { 36 } else { 34 }..64]
            .iter()
            .any(|&b| b != 0)
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
    let len = u16_at(bytes, 32) as usize
        + if mode != 4 || profile.id == 5 {
            (u16_at(bytes, 34) as usize) << 16
        } else {
            0
        };
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
                color_mode: mode,
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
            color_mode: mode,
            kind,
            transfer_id: bytes[8..24].try_into().unwrap(),
            block_index: index,
            total_blocks: total,
            payload_len: len as u32,
        },
        payload: payload.to_vec(),
        metadata,
    })
}

/// Inverse of the JS symbol packer; white (4) is rejected, never mapped to black.
pub fn unpack_symbols(symbols: &[u8]) -> Result<Vec<u8>> {
    if !PROFILES
        .iter()
        .any(|p| symbols.len() == p.matrix_width * p.matrix_height)
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

/// VMT2 symbol stream. Indexed modes are packed most-significant-bit first;
/// RGB888 stores exactly three channel bytes per data symbol.
pub fn unpack_symbols_mode(symbols: &[u32], profile: Profile, mode: u8) -> Result<Vec<u8>> {
    let Some(size) = profile.frame_size_for(mode) else {
        return Err(Error::Invalid("color mode"));
    };
    if symbols.len() != profile.matrix_width * profile.matrix_height {
        return Err(Error::Invalid("symbol count"));
    }
    let mut out = vec![0u8; size];
    if mode == 24 {
        for (i, &symbol) in symbols.iter().enumerate() {
            if symbol > 0xffffff {
                return Err(Error::Invalid("RGB symbol"));
            }
            out[i * 3] = (symbol >> 16) as u8;
            out[i * 3 + 1] = (symbol >> 8) as u8;
            out[i * 3 + 2] = symbol as u8;
        }
    } else {
        let bits = match mode {
            2 => 1,
            4 => 2,
            8 => 3,
            16 => 4,
            _ => unreachable!(),
        };
        for (i, &symbol) in symbols.iter().enumerate() {
            if symbol >= (1 << bits) {
                return Err(Error::Invalid("data symbol"));
            }
            for j in 0..bits {
                let bit = i * bits + j;
                out[bit / 8] |= (((symbol >> (bits - 1 - j)) & 1) << (7 - bit % 8)) as u8;
            }
        }
    }
    Ok(out)
}
