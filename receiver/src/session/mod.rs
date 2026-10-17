//! One-file, in-process collector. Image errors are discarded before they reach
//! this module; CRC-valid semantic errors are checked again against metadata.
use crate::{
    metrics::Metrics,
    protocol::{FrameType, Metadata, TransferId, ValidatedFrame},
    storage::OutputFile,
    Error, Result,
};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiveState {
    WaitingMetadata,
    Collecting,
    Verifying,
    Complete(PathBuf),
    Failed,
}
#[derive(Debug, PartialEq, Eq)]
pub enum SessionEvent {
    WaitingMetadata,
    Initialized,
    MetadataRepeat,
    ForeignTransfer,
    Duplicate,
    BlockAccepted(u32),
    Complete(PathBuf),
    AlreadyComplete,
}

/// Only the active session owns a temporary file. Missing blocks persist across
/// loops but never across process restarts. No feedback or missing-list input API.
pub struct ReceiveSession {
    state: ReceiveState,
    directory: PathBuf,
    identity: Option<TransferId>,
    metadata: Option<Metadata>,
    /// Packed presence bits: at most 33328 bytes at the v1 file-size limit.
    received: Vec<u64>,
    output: Option<OutputFile>,
    pub metrics: Metrics,
}
impl ReceiveSession {
    pub fn new(output_directory: &Path) -> Self {
        Self {
            state: ReceiveState::WaitingMetadata,
            directory: output_directory.into(),
            identity: None,
            metadata: None,
            received: Vec::new(),
            output: None,
            metrics: Metrics::default(),
        }
    }
    pub fn state(&self) -> &ReceiveState {
        &self.state
    }
    /// Returns false on success or terminal failure; a capture loop must then stop.
    pub fn is_consuming(&self) -> bool {
        matches!(
            self.state,
            ReceiveState::WaitingMetadata | ReceiveState::Collecting
        )
    }
    /// Only accepts parsed CRC-valid frames. Invalid semantic frames are discarded;
    /// conflict, I/O and final hash errors destroy temporary output and fail the session.
    pub fn ingest(&mut self, frame: &ValidatedFrame) -> Result<SessionEvent> {
        if matches!(self.state, ReceiveState::Complete(_)) {
            return Ok(SessionEvent::AlreadyComplete);
        }
        if self.state == ReceiveState::Failed {
            return Err(Error::FailedSession);
        }
        self.metrics.counters.valid_frames += 1;
        let result = self.ingest_inner(frame);
        if let Err(ref error) = result {
            if matches!(error, Error::Invalid(_)) {
                self.metrics.counters.invalid_frames += 1;
            } else {
                self.state = ReceiveState::Failed;
                self.output.take();
            }
        }
        result
    }
    fn ingest_inner(&mut self, frame: &ValidatedFrame) -> Result<SessionEvent> {
        let header = frame.header();
        if self.identity.is_some_and(|id| id != header.transfer_id) {
            self.metrics.counters.foreign_frames += 1;
            return Ok(SessionEvent::ForeignTransfer);
        }
        if let Some(meta) = frame.metadata() {
            if let Some(existing) = &self.metadata {
                if existing != meta {
                    return Err(Error::Conflict);
                }
                return Ok(SessionEvent::MetadataRepeat);
            }
            let output = OutputFile::create(&self.directory, &meta.filename)?;
            self.received = vec![0; (header.total_blocks as usize).div_ceil(64)];
            self.identity = Some(header.transfer_id);
            self.metadata = Some(meta.clone());
            self.output = Some(output);
            self.metrics.counters.total_blocks = header.total_blocks;
            self.state = ReceiveState::Collecting;
            return if header.total_blocks == 0 {
                self.complete()
            } else {
                Ok(SessionEvent::Initialized)
            };
        }
        if header.kind != FrameType::Data {
            return Err(Error::Invalid("unexpected frame type"));
        }
        let Some(meta) = &self.metadata else {
            self.metrics.counters.waiting_metadata += 1;
            return Ok(SessionEvent::WaitingMetadata);
        };
        if header.profile.block_size() != meta.block_size as usize
            || header.total_blocks != meta.total_blocks()
            || meta.block_len(header.block_index) != Some(frame.payload().len())
        {
            return Err(Error::Invalid("block does not match metadata"));
        }
        let index = header.block_index as usize;
        let offset = index as u64 * meta.block_size as u64;
        let output = self
            .output
            .as_mut()
            .expect("collecting session owns output");
        let word = index / 64;
        let bit = 1u64 << (index % 64);
        if self.received[word] & bit != 0 {
            if !output.block_matches(offset, frame.payload())? {
                return Err(Error::Conflict);
            }
            self.metrics.counters.duplicates += 1;
            return Ok(SessionEvent::Duplicate);
        }
        output.write_block(offset, frame.payload())?;
        self.received[word] |= bit;
        self.metrics.counters.unique_blocks += 1;
        self.metrics.counters.unique_bytes += frame.payload().len() as u64;
        if self.metrics.counters.unique_blocks == header.total_blocks {
            self.complete()
        } else {
            Ok(SessionEvent::BlockAccepted(header.block_index))
        }
    }
    fn complete(&mut self) -> Result<SessionEvent> {
        self.state = ReceiveState::Verifying;
        let path = self
            .output
            .take()
            .expect("verifying owns output")
            .publish(self.metadata.as_ref().unwrap())?;
        self.state = ReceiveState::Complete(path.clone());
        Ok(SessionEvent::Complete(path))
    }
}
