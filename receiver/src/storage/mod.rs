//! Disk-backed random access and verified, atomic publication. Never executes data.
use crate::{protocol::Metadata, Error, Result};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static TEMP_SERIAL: AtomicU64 = AtomicU64::new(0);

/// Convert an untrusted cross-platform filename to one Debian basename.
pub fn safe_basename(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let mut clean: String = base
        .chars()
        .map(|c| if c.is_control() { '_' } else { c })
        .collect();
    if clean.is_empty() || clean == "." || clean == ".." {
        clean = "received.bin".into();
    }
    let mut end = clean.len().min(255);
    while !clean.is_char_boundary(end) {
        end -= 1;
    }
    clean.truncate(end);
    clean
}

/// A private temporary file in the selected destination directory. Drop removes
/// incomplete output. Publication consumes this object, preventing a second save.
pub struct OutputFile {
    file: File,
    temporary: PathBuf,
    destination: PathBuf,
}
impl OutputFile {
    pub fn create(directory: &Path, filename: &str) -> Result<Self> {
        let directory = directory.canonicalize()?;
        let destination = directory.join(safe_basename(filename));
        // Fast diagnostic only; renameat2 below is the actual race-free guard.
        if destination.symlink_metadata().is_ok() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "output already exists",
            )
            .into());
        }
        for _ in 0..128 {
            let serial = TEMP_SERIAL.fetch_add(1, Ordering::Relaxed);
            let temporary = directory.join(format!(".vmt-{}-{serial}.part", std::process::id()));
            if temporary == destination {
                continue;
            }
            match OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)
            {
                Ok(file) => {
                    return Ok(Self {
                        file,
                        temporary,
                        destination,
                    })
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.into()),
            }
        }
        Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "temporary filename exhaustion",
        )
        .into())
    }
    /// Call only after metadata/index validation. Offset is bounded by v1 file size.
    pub fn write_block(&mut self, offset: u64, bytes: &[u8]) -> Result<()> {
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.write_all(bytes)?;
        Ok(())
    }
    /// Compare a repeated block to the stored bytes, detecting contradictory repeats.
    pub fn block_matches(&mut self, offset: u64, bytes: &[u8]) -> Result<bool> {
        let mut old = vec![0; bytes.len()];
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.read_exact(&mut old)?;
        Ok(old == bytes)
    }
    /// Stream length/hash validation, flush, and atomic rename with NOREPLACE.
    pub fn publish(mut self, metadata: &Metadata) -> Result<PathBuf> {
        if self.file.metadata()?.len() != metadata.file_size {
            return Err(Error::HashMismatch);
        }
        self.file.seek(SeekFrom::Start(0))?;
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 65536];
        loop {
            let n = self.file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        let digest: [u8; 32] = hash.finalize().into();
        if digest != metadata.sha256 {
            return Err(Error::HashMismatch);
        }
        self.file.sync_all()?;
        rustix::fs::renameat_with(
            rustix::fs::CWD,
            &self.temporary,
            rustix::fs::CWD,
            &self.destination,
            rustix::fs::RenameFlags::NOREPLACE,
        )
        .map_err(std::io::Error::from)?;
        Ok(self.destination.clone())
    }
}
impl Drop for OutputFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.temporary);
    }
}
