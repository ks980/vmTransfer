# VMT1 visual file protocol — version 1, profiles 1–3

Status: implemented predefined profiles; profile 1 remains byte-compatible. Live VNC viability is not yet measured.
All offsets and lengths below are bytes unless explicitly called symbols. No file
compression, feedback, repair codes or out-of-band file metadata are permitted.

## Binary frame

Frame size F and fixed payload capacity B are determined by a predefined profile:

| Profile ID | Data matrix N×N | Logical canvas (N+32)² | F bytes | B = F−68 bytes | Maximum blocks (1 GiB) |
|---|---|---|---:|---:|---:|
| 1 | 128×128 | 160×160 | 4096 | 4028 | 266570 |
| 2 | 160×160 | 192×192 | 6400 | 6332 | 169574 |
| 3 | 256×256 | 288×288 | 16384 | 16316 | 65810 |

The default is profile 1. A transfer keeps one profile for all frames and loops.
Changing the sender matrix prepares a new transfer ID; start a fresh receive session.
Every frame is exactly F bytes. Unsigned integers are little-endian. Unknown
versions, types, profiles, nonzero flags/reserved bytes and noncanonical padding
are rejected. Validate CRC before trusting any header-controlled allocation.

| Offset | Size | Field |
|---|---:|---|
| 0 | 4 | ASCII `VMT1` |
| 4 | 1 | version = 1 |
| 5 | 1 | type: metadata = 1, data = 2 |
| 6 | 1 | visual profile = 1, 2 or 3 |
| 7 | 1 | flags = 0 |
| 8 | 16 | opaque transfer ID, random for each new transfer |
| 24 | 4 | data block index, or `0xffffffff` for metadata |
| 28 | 4 | total data block count |
| 32 | 2 | payload length, 0..B |
| 34 | 30 | reserved = zero |
| 64 | B | payload followed by zero padding |
| F−4 | 4 | CRC-32/ISO-HDLC over bytes [0,F−4) |

CRC uses reflected polynomial `0xedb88320`, initial value `0xffffffff`, final
XOR `0xffffffff`, reflected input/output. Check: ASCII `123456789` -> `cbf43926`.
CRC detects accidental corruption; it is not authentication.

Metadata payload, in order: file length u64, block size u32 (=B for the frame profile), SHA-256
digest (32 raw bytes), filename byte length u16, filename (strict UTF-8, 1..1024
bytes). No trailing bytes inside the declared payload. SHA-256 covers the exact
original file bytes and is computed before the first frame is shown.

All profiles support files of 0..1073741824 bytes (1 GiB), with the maximum block
counts listed above. Count must equal ceil(file_length / B). Empty files have zero blocks,
only metadata frames, and the SHA-256 of the empty byte string. Data indices are
0..count-1; all but the last payload have B bytes. The last has exactly the
remaining length, never zero. Padding is not file content. Receivers check these
relations against accepted metadata before storing a block.

## Visual profile (known before decoding)

The image is an (N+32)×(N+32) logical-symbol square. Its background is white. Coordinates
start at the upper-left. Rectangles are half-open. The outermost four symbols are
white quiet zone. Display scale is independent of this fixed profile.

* Encoding area: [16,N+16) x [16,N+16), exactly N×N symbols.
* Three 7x7 finder squares start at (4,4), (N+21,4), (4,N+21). Each has a black
  one-symbol outer border, a white one-symbol inner border, and a black 3x3 center
  (the central horizontal/vertical runs are 1:1:3:1:1).
* Timing lines: (x,12), x=16..N+15 and (12,y), y=16..N+15; even coordinates black,
  odd coordinates white.
* Five 8x8 swatches start at (16,N+20), (32,N+20), (48,N+20), (64,N+20), (80,N+20),
  in order black, red, green, blue, white.
* Data palette: 00 black (#000000), 01 red (#ff0000), 10 green (#00ff00),
  11 blue (#0000ff). White is never a data symbol.
* Bytes traverse the encoding area row-major. Each byte emits bits 7..6, 5..4,
  3..2, 1..0 in that order. The encoding area includes the binary header and CRC.

Canvas rendering uses integer symbol sizes and disables smoothing. The decoder
must locate the three finders, recover orientation, and sample cell centers;
image dimensions or a file manifest must never substitute for detection. All profiles
support translation, independent horizontal/vertical scale and quarter-turns.
There is no bootstrap dependency on decoded metadata: try only the three predefined
profiles, with finder center separation N+17 logical symbols and 7-symbol finder
width. Validate timing, palette, CRC and agreement of image profile with byte 6.
Never accept arbitrary header-controlled dimensions. Frame allocations are at most
16384 bytes (65536 data symbols). The collector also rejects a data profile whose
block capacity differs from the accepted metadata, including short final blocks.
Camera perspective and arbitrary-angle rotation are outside this profile.

Use observed black/white swatches to normalize each RGB channel, and observed
data swatches for nearest-palette classification. Initial normalized Euclidean
distance limit is 0.35 and runner-up distance margin is 0.15. Swatch contrast
must be at least 80/255 per channel. These are implementation acceptance thresholds,
not encoded settings; changes require measured regression results. Geometry or
symbol mapping changes require a new profile/version and corresponding vectors.

## Sequence and receive session

A cycle contains metadata followed by each group of up to 64 sequential data
blocks. Thus metadata precedes blocks 0, 64, 128, etc. After the last block, start
the same cycle indefinitely. Empty transfers repeat metadata. Frame identity is
(transfer ID, frame type, block index), not wall-clock time or loop number.
Repetitions have byte-identical frames. Stop/pause does not change the transfer ID;
preparing a new transfer does. Fixtures inject a fixed ID for reproducibility.

Before valid metadata, ignore data. First valid metadata locks the session ID and
all metadata fields. Foreign IDs are explicitly counted/rejected, without resetting
the active session. Repeated identical metadata is harmless. Conflicting metadata
for the same ID or different valid payloads at an already stored index are fatal.
CRC failures never enter the collector. Other malformed frames are rejected.

Blocks can arrive in any order and over arbitrarily many cycles in one process.
Store only unique valid blocks. On completion verify actual file length and SHA-256,
then atomically publish once and stop consuming frames. Hash failure is terminal;
start a new receive session to retry. No restart resume is provided.

Received filenames are untrusted: take the last component using both '/' and '\\',
replace control characters, replace empty/dot/dot-dot names with `received.bin`,
and truncate at a UTF-8 boundary to 255 bytes. Never overwrite an existing output.
Temporary storage lives in the selected output directory and is removed on normal
failure/drop. Process crash may leave an untrusted `.vmt-*.part` file; it is never
automatically resumed or presented as a completed transfer.

Mixed old/new display updates may decode as an unchanged old/new frame (valid), or
as inconsistent bytes (reject via CRC). CRC is not collision-proof: final SHA-256
is mandatory and no unchecked output is ever published.
