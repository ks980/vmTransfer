# AGENTS.md — Visual File Transfer over VNC

## 1. Mission

Build a **one-way file transfer system over the visual channel of an existing VNC remote-desktop session**. The remote sender runs on **Windows 7 in a browser**; the local receiver runs as a **native application on Debian GNOME Wayland**. The sender renders QR-like colored data matrices; the receiver captures the VNC client's displayed image, decodes frames, accumulates valid blocks across repeated broadcasts, and restores exactly one file.

This is **not** a camera-based scanner and **not** a network file-transfer application. Do not introduce sockets, clipboard transfer, VNC file-transfer features, shared folders, HTTP uploads, or other data channels between sender and receiver. Serving the sender's static web page locally is allowed; it is not a file data channel.

## 2. Non-negotiable requirements

- Sender: static HTML/CSS/**vanilla JavaScript** with Canvas; compatible with Windows 7's **Firefox 115 ESR**. No Electron, modern-browser-only APIs, or mandatory build tools on the target computer. Use external dependencies only if clearly justified.
- Receiver: **native Debian GNOME Wayland** application; **Rust is preferred**. Prefer **XDG Desktop Portal ScreenCast + PipeWire** with user-authorized capture of the VNC window or screen. Wayland does not permit arbitrary capture of other apps. Avoid reliance on X11-only APIs.
- Transport: pixels shown in the sender browser reach receiver via **VNC**, with potential scaling, lossy compression, latency, dropped/duplicated frames, and partial updates. Never assume lossless pixel equality or fixed physical resolution.
- UI display frame: QR-like **self-locating markers**, a fixed **logical matrix** initially **128×128 symbols** (provisional; measure viability), four data colors **red, green, blue, black** (2 bits/symbol), and black/white finder/quiet-zone patterns. The fifth color, white, is reserved for locator/spacing, not data. Color mapping is provisional until measured. No requirement for standard QR compatibility.
- The user can manually configure visual scale or window size and choose one of the discrete target rates: **5, 10, 15, 20, 25, 30 FPS**. The logical matrix dimensions stay fixed during a transfer; the actual onscreen pixel dimensions may vary.
- Transfer **one file per session**. Input is already compressed if the user wants compression. **Do not compress or decompress file data in this application.** Use arbitrary binary-safe input/output.
- Fixed-sized logical payload blocks; initially target one block per visual data frame (metadata may use a dedicated recurring frame).
- **Unidirectional, no automatic ACK/feedback**, no manual missing-frame entry. On reaching the last frame, sender **automatically loops from the beginning indefinitely** until manually stopped. All loop repetitions have identical transfer ID, block numbers and contents.
- Receiver accepts valid blocks in any order, deduplicates by index, ignores invalid CRC frames, and collects missing blocks across any number of loops **within a single running receive session**. No persistence/resume across receiver restarts for MVP.
- **CRC32** for each decoded frame and **SHA-256** for the fully reconstructed file. Do not publish a final file unless the length and hash match. Preserve original filename safely (sanitize to basename; prevent path traversal).
- Receiver automatically completes and stops consuming frames on success; sender keeps looping until the user stops it. On an unrecoverable hash mismatch, report failure and require a fresh receive session, not a corrupted output file.
- Do not add forward error correction, fountain codes, zstd, Deflate, adaptive-rate negotiation, or multi-file batches to MVP. Provide extension points but do not implement unnecessary features.

## 3. Architecture

```text
Windows 7 / Firefox 115 ESR               Existing VNC                  Debian / GNOME Wayland
File -> JS block encoder -> Canvas  === pixel/video updates ===>  Portal+PipeWire capture
                                                                    -> locator/grid sampler
                                                                    -> palette decoder
                                                                    -> frame CRC32
                                                                    -> index-based collector
                                                                    -> file size + SHA-256
                                                                    -> atomic output save
```

Split the repo into:

- `sender/`: static, offline-capable web UI, encoder, canvas renderer, file/session logic.
- `receiver/`: Rust application, Wayland capture adapter, frame decoder, block collector, file writer, metrics.
- `protocol/`: human-readable specification, fixed test vectors, color/geometry definitions, frame layout.
- `tests/`: cross-language fixtures, synthetic frame tests, and repeatable performance measurements.
- `docs/`: setup for Windows 7 and Debian Wayland; limitations and manual benchmark procedure.

These are suggested paths; retain a simple structure and document changes.

## 4. Wire/image protocol design constraints

Before implementing full transfer, write `protocol/SPEC.md` documenting an explicit, versioned **binary** frame format and its visual mapping. Sender and receiver must implement exactly the same format. Include at minimum:

- protocol version and frame type (`metadata` or `data`);
- transfer/session ID distinguishing new transfers from stale images;
- block index (data frames), total block count, payload byte length;
- frame CRC32 over precisely specified bytes (including covered header fields);
- metadata: original file basename, total file length, SHA-256, fixed payload block size.

Define byte order, sizes, alignment/padding, CRC variant (e.g. CRC-32/ISO-HDLC), how the final short block is handled, UTF-8 filename treatment, maximum supported file size/block count, and deterministic frame numbering. Decide whether a whole transfer's SHA-256 is computed before playback starts; if so show progress and handle cancellation. Keep the metadata frame repeated in each cycle so a receiver joining late can initialize. Validate a new ID and metadata before accepting blocks.

**Avoid bootstrap circularity:** the receiver must locate and know how to parse the frame grid/header *before* decoding configuration from it. Use fixed high-contrast markers and a fixed low-density bootstrap region or one predefined grid profile. Receiver must tolerate translation, rescaling, and letterboxing. Ideally avoid assuming physical RGB equality; classify colors via reference swatches/nearest-palette with measured thresholds. All decoded frames still need CRC confirmation.

**Avoid mixed-frame corruption:** a screenshot may contain partially updated old and new content. The CRC must reject inconsistent frames; consider repeating frame ID/header at multiple locations if testing indicates it is necessary. Use nearest-neighbor rendering (no smoothing) and integer symbol sizes where feasible.

**Transfer ID semantics:** one ID for all loops of a loaded file; regenerate only on a new transfer. A metadata frame must be available periodically, including after startup and on each loop. Receiver should not silently merge frames from different IDs. If a different transfer appears mid-session, explicitly reject it or request the user to reset.

## 5. Sender behavior

- Select one arbitrary local file through `<input type="file">`; read with File/Blob APIs in a binary-safe way.
- Compute SHA-256 and assemble metadata. Avoid loading huge input files into unnecessary duplicate buffers; use chunked access as practical.
- Expose start, pause/stop, rate selector, and manual visual scale/size settings. Show file name, size, frame index/total, current loop count, requested FPS, and preparation/errors.
- Render repeated metadata plus sequential fixed-size data blocks, then immediately start the next cycle. Preserve stable block indexes and byte-identical payloads across loops.
- Use scheduling that does not assume `requestAnimationFrame` equals actual VNC delivery rate. Measure local frame timing. Accept that background tab throttling can occur; document it.
- Ensure usable page behavior in Firefox 115 ESR. Do not require WebGPU, WebCodecs, modern `getDisplayMedia`, or unsupported JS syntax.

## 6. Receiver behavior

- User authorizes capture of the VNC client window/screen through the GNOME portal; obtain video buffers via PipeWire. Provide explicit guidance when permission is denied or unavailable.
- For development/testing, support reading saved PNG frames or a supplied in-memory bitmap through the **same decoder path**. This is essential for deterministic tests before live capture works.
- Locate markers, estimate the matrix bounds and orientation, sample symbol centers, classify 4 colors, parse frame, and check CRC.
- Maintain a receive-session state table keyed by block index; only count a block after CRC succeeds. Deduplicate repeats and reject incompatible metadata/transfer IDs.
- Show received/total blocks, unique valid frames per second, duplicates, CRC failures, estimated effective goodput, and elapsed time. Missing-block list is optional **for display only**, never needed as manual sender input.
- On all blocks received, reconstruct using exact block order, verify file length and SHA-256, and atomically rename a temporary file to a sanitized user-chosen output path. On mismatch report failure and do not leave a final corrupted file.
- Gracefully handle resizing, disconnection, repeated captured frames, unchanged frames, and portal/capture failures.

## 7. Implementation phases and acceptance criteria

### Phase 0 — inspect and design
1. Inspect repo and environment. Do not assume dependencies exist.
2. Write protocol specification, module interfaces, and a short implementation plan.
3. Establish supported Firefox APIs and Wayland capture approach; identify risky libraries before adopting them.

### Phase 1 — deterministic codec, **no VNC required**
1. Build sender encoder and a decoder operating on generated PNG/bitmap frames.
2. Include finder patterns, fixed bootstrap, color mapping, metadata and data frames, CRC32 and SHA-256.
3. Add golden test vectors shared across JS/Rust; test random binary, zero bytes, Unicode filenames, empty files, and last partial blocks.
4. Inject scaling, offsets, nearest/bilinear resampling, mild color distortion, duplicate, missing and mixed frames; reject bad CRC and reconstruct when all required blocks eventually arrive.

### Phase 2 — running sender and offline receiver
1. Provide working local HTML sender UI in Firefox 115 ESR.
2. Create CLI Rust receiver that decodes test PNGs/synthetic frame sequences and collects blocks over repeated loops.
3. Prove byte-for-byte recovery and SHA-256 validation without feedback.

### Phase 3 — real Wayland capture and VNC
1. Implement user-authorized ScreenCast portal + PipeWire integration; allow window/screen selection where supported.
2. Capture the existing VNC client image, crop/detect dynamically, and decode in real time.
3. Benchmark **5/10/15/20/25/30 FPS** and several physical symbol sizes. Report actual unique valid frames/s, symbol/frame error rates, CPU, and goodput.
4. Adjust palette thresholds/geometry only based on evidence; do not silently change the agreed protocol without versioning and corresponding test fixtures.

### Phase 4 — stabilization
1. Improve UI/error messages and documentation.
2. Confirm repeated loops fill holes automatically, and success auto-stops receiver while sender continues.
3. Document practical throughput limits and failure conditions.

**MVP is accepted when:** a single arbitrary binary file is sent by a Firefox 115 ESR sender over a VNC-displayed canvas, received by Debian GNOME Wayland native capture, recovered across missed/duplicate frames without feedback, SHA-256 matches, and saved once. Both sides display useful progress/errors. Do not claim a throughput figure without a measured benchmark.

## 8. Engineering quality rules for Codex

- Work in small, testable increments; run available tests/builds after material changes.
- Prefer working code over scaffolding. Never mark a feature complete when it is only a stub.
- Isolate transport-independent image codec from PipeWire and UI. Keep protocol parsing fuzz-friendly and bounds-checked; never allocate unbounded memory from untrusted header values.
- Use pure functions for symbol packing/unpacking, frame CRC checks, and index collection where possible.
- Add a concise README with exact development commands, Windows 7 deployment steps, required Debian dependencies/portal permissions, and known constraints.
- Record design changes in `docs/DECISIONS.md` with rationale. Ask for clarification only when a real blocking ambiguity arises; otherwise select a conservative default, document it, and implement.
- Do not assume Linux Wayland capture can silently read arbitrary windows. Do not claim 30 FPS reception just because sender schedules 30 FPS.
- Treat the VNC channel as possibly lossy and untrusted. Never execute received content; write data only to the selected output file.

## 9. Initial Codex task

Start by implementing **Phase 0 and Phase 1**. Produce the protocol spec and a working JS encoder + Rust offline decoder with reproducible PNG/bitmap test fixtures. Do **not** start with portal/PipeWire integration or full GUI before the end-to-end offline codec is demonstrably correct. Report exact commands run and test outcomes; identify the next blocker to live capture.
