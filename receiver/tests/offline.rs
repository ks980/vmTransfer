//! Independent Python vectors exercise Rust parsing, PNG detection and collection.
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use vmtransfer::{
    capture::{
        png::{load_png, PngSequence},
        FrameSource,
    },
    codec::{
        bitmap::{BitmapView, PixelFormat},
        decode_bitmap,
    },
    protocol::{parse_frame, unpack_symbols, ValidatedFrame},
    session::{ReceiveSession, ReceiveState, SessionEvent},
    storage::safe_basename,
    Error,
};

static SERIAL: AtomicU64 = AtomicU64::new(0);
struct TestDir(PathBuf);
impl TestDir {
    fn new() -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-output");
        fs::create_dir_all(&root).unwrap();
        let path = root.join(format!(
            "{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn empty(&self) -> bool {
        fs::read_dir(&self.0).unwrap().next().is_none()
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn vectors() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../protocol/vectors")
}
fn raw(case: &str, index: usize) -> Vec<u8> {
    fs::read(vectors().join(format!("{case}/frame-{index:03}.bin"))).unwrap()
}
fn frame(case: &str, index: usize) -> ValidatedFrame {
    parse_frame(&raw(case, index)).unwrap()
}
fn crc(mut bytes: Vec<u8>) -> Vec<u8> {
    let end = bytes.len() - 4;
    let hash = crc32fast::hash(&bytes[..end]);
    bytes[end..].copy_from_slice(&hash.to_le_bytes());
    bytes
}
fn mutate(case: &str, index: usize, offset: usize, value: u8) -> ValidatedFrame {
    let mut b = raw(case, index);
    b[offset] = value;
    parse_frame(&crc(b)).unwrap()
}

#[test]
fn all_golden_pngs_decode_and_recover_exactly() {
    let cases: serde_json::Value =
        serde_json::from_slice(&fs::read(vectors().join("cases.json")).unwrap()).unwrap();
    for case in cases.as_array().unwrap() {
        let dir = TestDir::new();
        let mut session = ReceiveSession::new(&dir.0);
        for (image, encoded) in case["images"]
            .as_array()
            .unwrap()
            .iter()
            .zip(case["frames"].as_array().unwrap())
        {
            let bitmap = load_png(&vectors().join(image.as_str().unwrap())).unwrap();
            let decoded =
                decode_bitmap(bitmap.view().unwrap()).unwrap_or_else(|e| panic!("{image}: {e}"));
            let expected =
                parse_frame(&fs::read(vectors().join(encoded.as_str().unwrap())).unwrap()).unwrap();
            assert_eq!(decoded.header(), expected.header());
            assert_eq!(decoded.payload(), expected.payload());
            session.ingest(&decoded).unwrap();
        }
        let ReceiveState::Complete(path) = session.state() else {
            panic!("incomplete {case}");
        };
        assert_eq!(
            fs::read(path).unwrap(),
            fs::read(vectors().join(case["input"].as_str().unwrap())).unwrap()
        );
        assert!(!session.is_consuming());
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
        assert_eq!(
            session
                .ingest(&frame(case["case"].as_str().unwrap(), 0))
                .unwrap(),
            SessionEvent::AlreadyComplete
        );
    }
}

#[test]
fn standard_hash_and_symbol_vectors() {
    assert_eq!(crc32fast::hash(b"123456789"), 0xcbf43926);
    assert_eq!(
        format!("{:x}", Sha256::digest(b"abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let mut symbols = [0; 128 * 128];
    symbols[..4].copy_from_slice(&[0, 1, 2, 3]);
    assert_eq!(unpack_symbols(&symbols).unwrap()[0], 0x1b);
    symbols[40] = 4;
    assert!(unpack_symbols(&symbols).is_err());
    assert!(unpack_symbols(&[]).is_err());
}

#[test]
fn presence_bitmap_crosses_word_boundaries_with_out_of_order_blocks() {
    let data: Vec<u8> = (0..4028 * 65 + 3).map(|i| (i % 251) as u8).collect();
    let count = data.len().div_ceil(4028) as u32;
    let mut meta = raw("random", 0);
    meta[28..32].copy_from_slice(&count.to_le_bytes());
    meta[64..72].copy_from_slice(&(data.len() as u64).to_le_bytes());
    meta[76..108].copy_from_slice(&Sha256::digest(&data));
    let dir = TestDir::new();
    let mut session = ReceiveSession::new(&dir.0);
    session.ingest(&parse_frame(&crc(meta)).unwrap()).unwrap();
    for index in (0..count).rev() {
        let mut block = raw("random", 1);
        let start = index as usize * 4028;
        let payload = &data[start..data.len().min(start + 4028)];
        block[24..28].copy_from_slice(&index.to_le_bytes());
        block[28..32].copy_from_slice(&count.to_le_bytes());
        block[32..34].copy_from_slice(&(payload.len() as u16).to_le_bytes());
        block[64..4092].fill(0);
        block[64..64 + payload.len()].copy_from_slice(payload);
        let frame = parse_frame(&crc(block)).unwrap();
        session.ingest(&frame).unwrap();
        if index == 64 {
            assert_eq!(session.ingest(&frame).unwrap(), SessionEvent::Duplicate);
        }
    }
    assert_eq!(session.metrics.counters.unique_blocks, count);
    assert_eq!(fs::read(dir.0.join("random.bin")).unwrap(), data);
}

#[test]
fn metadata_size_limit_is_checked_before_storage() {
    let mut meta = raw("random", 0);
    meta[28..32].copy_from_slice(&266570u32.to_le_bytes());
    meta[64..72].copy_from_slice(&(1u64 << 30).to_le_bytes());
    let parsed = parse_frame(&crc(meta.clone())).unwrap();
    assert_eq!(parsed.metadata().unwrap().total_blocks(), 266570);
    assert_eq!(parsed.metadata().unwrap().block_len(266569), Some(1892));
    meta[64..72].copy_from_slice(&((1u64 << 30) + 1).to_le_bytes());
    assert!(parse_frame(&crc(meta)).is_err());
}

#[test]
fn missed_frames_are_filled_across_loops_without_feedback() {
    let dir = TestDir::new();
    let mut s = ReceiveSession::new(&dir.0);
    assert_eq!(
        s.ingest(&frame("random", 4)).unwrap(),
        SessionEvent::WaitingMetadata
    );
    assert!(dir.empty());
    s.ingest(&frame("random", 0)).unwrap();
    for i in [4, 2, 4, 0, 2] {
        s.ingest(&frame("random", i)).unwrap();
    }
    assert_eq!(s.metrics.counters.unique_blocks, 2);
    assert_eq!(s.metrics.counters.duplicates, 2);
    assert!(s.is_consuming());
    for i in [0, 1, 3] {
        s.ingest(&frame("random", i)).unwrap();
    }
    assert_eq!(
        fs::read(dir.0.join("random.bin")).unwrap(),
        fs::read(vectors().join("random/input.bin")).unwrap()
    );
}

#[test]
fn foreign_transfer_is_rejected_without_resetting_active_transfer() {
    let dir = TestDir::new();
    let mut s = ReceiveSession::new(&dir.0);
    s.ingest(&frame("random", 0)).unwrap();
    assert_eq!(
        s.ingest(&mutate("random", 0, 8, 99)).unwrap(),
        SessionEvent::ForeignTransfer
    );
    assert_eq!(
        s.ingest(&mutate("random", 1, 8, 99)).unwrap(),
        SessionEvent::ForeignTransfer
    );
    assert_eq!(s.metrics.counters.foreign_frames, 2);
    assert_eq!(s.metrics.counters.unique_blocks, 0);
    for i in 1..=4 {
        s.ingest(&frame("random", i)).unwrap();
    }
    assert!(!s.is_consuming());
}

#[test]
fn conflicting_metadata_fails_and_cleans_temporary_file() {
    let dir = TestDir::new();
    let mut s = ReceiveSession::new(&dir.0);
    s.ingest(&frame("random", 0)).unwrap();
    assert!(matches!(
        s.ingest(&mutate("random", 0, 76, 99)),
        Err(Error::Conflict)
    ));
    assert_eq!(s.state(), &ReceiveState::Failed);
    assert!(dir.empty());
    assert!(matches!(
        s.ingest(&frame("random", 0)),
        Err(Error::FailedSession)
    ));
}

#[test]
fn contradictory_duplicate_fails_even_when_crc_valid() {
    let dir = TestDir::new();
    let mut s = ReceiveSession::new(&dir.0);
    s.ingest(&frame("random", 0)).unwrap();
    s.ingest(&frame("random", 1)).unwrap();
    let mut b = raw("random", 1);
    b[100] ^= 1;
    assert!(matches!(
        s.ingest(&parse_frame(&crc(b)).unwrap()),
        Err(Error::Conflict)
    ));
    assert!(dir.empty());
}

#[test]
fn metadata_length_checks_discard_incompatible_data_without_losing_session() {
    let dir = TestDir::new();
    let mut s = ReceiveSession::new(&dir.0);
    s.ingest(&frame("after-boundary", 0)).unwrap();
    let mut short = raw("after-boundary", 2);
    short[32] = 2;
    assert!(matches!(
        s.ingest(&parse_frame(&crc(short)).unwrap()),
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        s.ingest(&mutate("after-boundary", 1, 28, 3)),
        Err(Error::Invalid(_))
    ));
    assert_eq!(s.metrics.counters.unique_blocks, 0);
    assert!(s.is_consuming());
    s.ingest(&frame("after-boundary", 1)).unwrap();
    s.ingest(&frame("after-boundary", 2)).unwrap();
    assert!(matches!(s.state(), ReceiveState::Complete(_)));
}

#[test]
fn wrong_final_sha_is_terminal_and_never_publishes() {
    let dir = TestDir::new();
    let mut s = ReceiveSession::new(&dir.0);
    s.ingest(&mutate("one", 0, 76, 99)).unwrap();
    assert!(matches!(
        s.ingest(&frame("one", 1)),
        Err(Error::HashMismatch)
    ));
    assert_eq!(s.state(), &ReceiveState::Failed);
    assert!(dir.empty());
}

#[test]
fn wrong_empty_sha_fails_without_output() {
    let dir = TestDir::new();
    let mut s = ReceiveSession::new(&dir.0);
    assert!(matches!(
        s.ingest(&mutate("empty", 0, 76, 99)),
        Err(Error::HashMismatch)
    ));
    assert!(dir.empty());
}

#[test]
fn existing_destination_is_never_overwritten_including_publication_race() {
    let dir = TestDir::new();
    let destination = dir.0.join("one.bin");
    fs::write(&destination, b"keep").unwrap();
    let mut s = ReceiveSession::new(&dir.0);
    assert!(matches!(s.ingest(&frame("one", 0)), Err(Error::Io(_))));
    assert_eq!(fs::read(&destination).unwrap(), b"keep");
    fs::remove_file(&destination).unwrap();
    let mut s = ReceiveSession::new(&dir.0);
    s.ingest(&frame("one", 0)).unwrap();
    fs::write(&destination, b"race").unwrap();
    assert!(matches!(s.ingest(&frame("one", 1)), Err(Error::Io(_))));
    assert_eq!(fs::read(&destination).unwrap(), b"race");
    assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
}

#[test]
fn interrupted_session_cleans_up_and_does_not_resume() {
    let dir = TestDir::new();
    {
        let mut s = ReceiveSession::new(&dir.0);
        s.ingest(&frame("random", 0)).unwrap();
        s.ingest(&frame("random", 1)).unwrap();
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
    }
    assert!(dir.empty());
    let mut s = ReceiveSession::new(&dir.0);
    assert_eq!(
        s.ingest(&frame("random", 2)).unwrap(),
        SessionEvent::WaitingMetadata
    );
}

#[test]
fn filesystem_errors_fail_cleanly() {
    let dir = TestDir::new();
    let not_dir = dir.0.join("not-a-directory");
    fs::write(&not_dir, b"keep").unwrap();
    let mut s = ReceiveSession::new(&not_dir);
    assert!(matches!(s.ingest(&frame("one", 0)), Err(Error::Io(_))));
    assert_eq!(s.state(), &ReceiveState::Failed);
    assert_eq!(fs::read(not_dir).unwrap(), b"keep");
}

#[test]
fn filenames_cannot_escape_output_directory() {
    for name in [
        "../../escape.bin",
        "C:\\Windows\\escape.bin",
        "../\\escape.bin",
    ] {
        assert_eq!(safe_basename(name), "escape.bin");
    }
    for name in ["", ".", "..", "../"] {
        assert_eq!(safe_basename(name), "received.bin");
    }
    assert_eq!(safe_basename("a\0b\nbin"), "a_b_bin");
    let long = safe_basename(&"中".repeat(200));
    assert!(long.len() <= 255);
    assert!(!long.is_empty());
    // Exercise actual metadata-to-publication path with a traversal filename.
    let dir = TestDir::new();
    let mut bytes = raw("one", 0);
    let name = b"../../escape.bin";
    bytes[110..4092].fill(0);
    bytes[108..110].copy_from_slice(&(name.len() as u16).to_le_bytes());
    bytes[110..110 + name.len()].copy_from_slice(name);
    bytes[32..34].copy_from_slice(&((46 + name.len()) as u16).to_le_bytes());
    let mut s = ReceiveSession::new(&dir.0);
    s.ingest(&parse_frame(&crc(bytes)).unwrap()).unwrap();
    s.ingest(&frame("one", 1)).unwrap();
    assert_eq!(fs::read(dir.0.join("escape.bin")).unwrap(), [0]);
}

#[test]
fn parser_rejects_corruption_and_noncanonical_fields() {
    for offset in [0, 4, 5, 6, 7, 8, 24, 28, 32, 34, 64, 4091, 4092] {
        let mut b = raw("one", 0);
        b[offset] ^= 1;
        assert!(matches!(parse_frame(&b), Err(Error::Crc)));
    }
    for (offset, value) in [
        (4, 2),
        (5, 3),
        (6, 2),
        (7, 1),
        (34, 1),
        (32, 255),
        (33, 255),
        (1000, 1),
        (71, 1),
        (72, 0),
        (110, 255),
    ] {
        let mut b = raw("one", 0);
        b[offset] = value;
        assert!(parse_frame(&crc(b)).is_err(), "offset {offset}");
    }
    for length in [0, 1, 34, 64, 4095, 4097, 10000] {
        assert!(parse_frame(&vec![0; length]).is_err());
    }
    // Random CRC-valid headers exercise post-CRC bounds checks without panics/allocations.
    let mut seed = 42u32;
    for _ in 0..1000 {
        let mut b = raw("random", 1);
        for byte in &mut b[24..34] {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            *byte = (seed >> 24) as u8;
        }
        let _ = parse_frame(&crc(b));
    }
}

#[test]
fn bitmap_formats_stride_and_limits_share_decoder() {
    let bitmap = load_png(&vectors().join("one/frame-000.png")).unwrap();
    let stride = bitmap.width * 4 + 32;
    let mut bytes = vec![222; stride * bitmap.height];
    for y in 0..bitmap.height {
        for x in 0..bitmap.width {
            let p = (y * bitmap.width + x) * 4;
            let q = y * stride + x * 4;
            bytes[q..q + 4].copy_from_slice(&[
                bitmap.rgba[p + 2],
                bitmap.rgba[p + 1],
                bitmap.rgba[p],
                0,
            ]);
        }
    }
    let view = BitmapView::new(
        bitmap.width,
        bitmap.height,
        stride,
        PixelFormat::Bgrx8,
        &bytes,
    )
    .unwrap();
    assert_eq!(
        decode_bitmap(view).unwrap().header(),
        frame("one", 0).header()
    );
    for (w, h, s) in [
        (0, 1, 4),
        (1, 0, 4),
        (8193, 1, 32772),
        (4, 4, 1),
        (1, 2, usize::MAX),
    ] {
        assert!(BitmapView::new(w, h, s, PixelFormat::Rgba8, &[0; 4]).is_err());
    }
}

#[test]
fn png_source_stops_and_rejects_malformed_input() {
    let mut source = PngSequence::new(vec![vectors().join("one/frame-000.png")]);
    source.stop().unwrap();
    assert!(source.next_frame().unwrap().is_none());
    let dir = TestDir::new();
    let path = dir.0.join("bad.png");
    fs::write(&path, b"not PNG").unwrap();
    assert!(load_png(&path).is_err());
}

#[test]
fn diagnostic_snapshot_roundtrips_and_refuses_overwrite() {
    use vmtransfer::capture::png::save_png;
    let dir = TestDir::new();
    let original = load_png(&vectors().join("one/frame-000.png")).unwrap();
    let path = dir.0.join("snapshot.png");
    save_png(&path, &original).unwrap();
    let loaded = load_png(&path).unwrap();
    assert_eq!(original.rgba, loaded.rgba);
    assert_eq!(
        decode_bitmap(loaded.view().unwrap()).unwrap().header(),
        frame("one", 0).header()
    );
    let before = fs::read(&path).unwrap();
    assert!(matches!(save_png(&path, &original), Err(Error::Io(_))));
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn profile_mismatch_is_rejected_before_storage_even_for_a_short_single_block() {
    let dir = TestDir::new();
    let mut session = ReceiveSession::new(&dir.0);
    session.ingest(&frame("one", 0)).unwrap();
    // Same ID, count, index and one-byte payload; only the profile differs.
    let old = raw("one", 1);
    let mut larger = vec![0u8; 6400];
    larger[..65].copy_from_slice(&old[..65]);
    larger[6] = 2;
    let foreign_profile = parse_frame(&crc(larger)).unwrap();
    assert!(matches!(
        session.ingest(&foreign_profile),
        Err(Error::Invalid(_))
    ));
    assert_eq!(session.metrics.counters.unique_blocks, 0);
    assert!(session.is_consuming());
    session.ingest(&frame("one", 1)).unwrap();
    assert!(matches!(session.state(), ReceiveState::Complete(_)));
}

#[test]
fn parser_rejects_profile_length_and_block_capacity_mismatches() {
    for case in ["one", "profile-160-partial", "profile-256-partial"] {
        let mut bytes = raw(case, 0);
        bytes[6] = if bytes[6] == 1 { 2 } else { 1 };
        assert!(matches!(parse_frame(&crc(bytes)), Err(Error::Invalid(_))));
        let mut bytes = raw(case, 0);
        bytes[72..76].copy_from_slice(&1u32.to_le_bytes());
        assert!(matches!(parse_frame(&crc(bytes)), Err(Error::Invalid(_))));
    }
}
