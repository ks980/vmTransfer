# VMT visual file protocol — versions 1 and 2, geometry profiles 1–7

Status: version 1 four-color frames remain byte-compatible; version 2 adds fixed color modes.
All offsets and lengths below are bytes unless explicitly called symbols. No file
compression, feedback, repair codes or out-of-band file metadata are permitted.

## Binary frame

For version 1, frame size F and fixed payload capacity B are determined by a predefined profile:

| Profile ID | Data matrix W×H | Logical canvas (W+32)×(H+32) | F bytes | B = F−68 bytes | Maximum blocks (1 GiB) |
|---|---|---|---:|---:|---:|
| 1 | 128×128 | 160×160 | 4096 | 4028 | 266570 |
| 2 | 160×160 | 192×192 | 6400 | 6332 | 169574 |
| 3 | 256×256 | 288×288 | 16384 | 16316 | 65810 |
| 4 | 512×512 | 544×544 | 65536 | 65468 | 16402 |
| 5 | 1028×512 | 1060×544 | 131584 | 131516 | 8165 |
| 6 | 512×256 | 544×288 | 32768 | 32700 | 32837 |
| 7 | 256×128 | 288×160 | 8192 | 8124 | 132170 |

The default is version 1 profile 1. A transfer keeps one geometry and color mode for all frames and loops.
Changing the sender matrix prepares a new transfer ID; start a fresh receive session.
Every frame is exactly F bytes. Unsigned integers are little-endian. Unknown
versions, types, profiles, nonzero flags/reserved bytes and noncanonical padding
are rejected. Validate CRC before trusting any header-controlled allocation.

| Offset | Size | Field |
|---|---:|---|
| 0 | 4 | ASCII `VMT1` |
| 4 | 1 | version = 1 |
| 5 | 1 | type: metadata = 1, data = 2 |
| 6 | 1 | visual profile = 1, 2, 3, 4, 5, 6 or 7 |
| 7 | 1 | flags = 0 |
| 8 | 16 | opaque transfer ID, random for each new transfer |
| 24 | 4 | data block index, or `0xffffffff` for metadata |
| 28 | 4 | total data block count |
| 32 | 2 | payload length low 16 bits |
| 34 | 2 | payload length high 16 bits for profile 5; zero for profiles 1–4, 6 and 7 |
| 36 | 28 | reserved = zero |
| 64 | B | payload followed by zero padding |
| F−4 | 4 | CRC-32/ISO-HDLC over bytes [0,F−4) |

CRC uses reflected polynomial `0xedb88320`, initial value `0xffffffff`, final
XOR `0xffffffff`, reflected input/output. Check: ASCII `123456789` -> `cbf43926`.
CRC detects accidental corruption; it is not authentication.
For profile 5, length = u16LE(32) + 65536 × u16LE(34). It must be 0..131516.
For profiles 1–4, 6 and 7, bytes 34–35 must be zero and length = u16LE(32).
The extension is CRC-covered; all old profile 1–3 frames retain identical bytes.

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

The image is a (W+32)×(H+32) logical-symbol rectangle. Its background is white. Coordinates
start at the upper-left. Rectangles are half-open. The outermost four symbols are
white quiet zone. Display scale is independent of this fixed profile.

* Encoding area: [16,W+16) x [16,H+16), exactly W×H symbols.
* Three 7x7 finder squares start at (4,4), (W+21,4), (4,H+21). Each has a black
  one-symbol outer border, a white one-symbol inner border, and a black 3x3 center
  (the central horizontal/vertical runs are 1:1:3:1:1).
* Timing lines: (x,12), x=16..W+15 and (12,y), y=16..H+15; even coordinates black,
  odd coordinates white.
* Five 8x8 swatches start at (16,H+20), (32,H+20), (48,H+20), (64,H+20), (80,H+20),
  in order black, red, green, blue, white.
* Version 1 data palette: 00 black (#000000), 01 red (#ff0000), 10 green (#00ff00),
  11 blue (#0000ff). Version 1 never uses white as a data symbol.
* Bytes traverse the encoding area row-major. Each byte emits bits 7..6, 5..4,
  3..2, 1..0 in that order. The encoding area includes the binary header and CRC.

Canvas rendering uses integer symbol sizes and disables smoothing. The decoder
must locate the three finders, recover orientation, and sample cell centers;
image dimensions or a file manifest must never substitute for detection. All profiles
support translation, independent horizontal/vertical scale and quarter-turns.
There is no bootstrap dependency on decoded metadata: try only the seven predefined
profiles, with finder center separation W+17 horizontally and H+17 vertically, and 7-symbol finder
width. Validate timing, palette, CRC and agreement of image profile with byte 6.
Never accept arbitrary header-controlled dimensions. Version 1 frame allocations are at most
131584 bytes (526336 data symbols); version 2 is capped at 1579008 bytes.
The collector also rejects a data profile whose
block capacity differs from the accepted metadata, including short final blocks.
Camera perspective and arbitrary-angle rotation are outside this profile.

Use observed black/white swatches to normalize each RGB channel, and observed
data swatches for nearest-palette classification. Initial normalized Euclidean
distance limit is 0.35 and runner-up distance margin is 0.15. Swatch contrast
must be at least 80/255 per channel. These are implementation acceptance thresholds,
not encoded settings; changes require measured regression results. Geometry or
symbol mapping changes require a new profile/version and corresponding vectors.

## Version 2 color modes

The sender may select 2, 8, 16 indexed colors or RGB888 (24 bits per symbol). Four colors
continue to use version 1. The three black/white finders, timing lines, geometry profiles,
frame types, transfer ID, CRC32, metadata payload, loop rules, 1 GiB file limit and final
SHA-256 check are unchanged. The four-color binary and PNG vectors remain unchanged.
Changing color mode prepares a new transfer ID; the receiver rejects mixing modes in one session.

The fixed bootstrap is known before reading any data header. Version 1 leaves cells
`(16..27,13)` white. Version 2 repeats one three-bit code four times across these twelve
cells, most significant bit first; black means 1 and white means 0. Codes 1, 2, 3 and 4
mean 2, 8, 16 and RGB888 respectively. Any disagreeing copy or unsupported code is rejected.
The receiver locates the old black/white geometry first, reads these cells, then samples
the data area with the selected mode. A CRC-valid header must agree with the bootstrap.

Version 2 keeps ASCII magic `VMT1` at bytes 0–3. Byte 4 is `2`; byte 6 is the unchanged
geometry profile ID; byte 7 is color mode `2`, `8`, `16` or `24`. Payload length is a u32
little-endian integer at bytes 32–35 for every version 2 profile. Bytes 36–63 are zero.
All header fields, payload and padding remain CRC-covered. There is no alignment between
payload and CRC: padding is zero through F−5. F is `W×H×bits_per_symbol/8`; RGB888 uses
24 bits, stored as R,G,B bytes in that order. B remains F−68. All seven geometry areas
are divisible by eight, so F is integral. An RGB888 1028×512 frame has F=1579008 and
B=1578940 bytes; implementations must cap the predefined allocation at this value.

Indexed symbols are packed row-major, most significant bit first, continuously across byte
boundaries. Index 0 is not special. Reference swatches are 6×6 symbols at
`(16+16×(i mod 9), H+18+7×floor(i/9))`. After all indexed colors, two extra swatches
contain black and white anchors. This fits at most eighteen swatches in two rows within
the smallest logical canvas. RGB888 uses five swatches at indices 0–4: black, red, green,
blue, white. Finder/quiet-zone black and white remain fixed in all modes. Version 2 data
may contain white; the fixed encoding area and bootstrap distinguish it from spacing.

| Mode | Bits/symbol | Indexed colors in symbol order |
|---|---:|---|
| 2 | 1 | `000000`, `ffffff` |
| 8 | 3 | `000000`, `ff0000`, `00ff00`, `0000ff`, `ffff00`, `ff00ff`, `00ffff`, `ffffff` |
| 16 | 4 | 8-color list, then `800000`, `008000`, `000080`, `808000`, `800080`, `008080`, `808080`, `ff8000` |
| RGB888 | 24 | Every symbol is three literal 8-bit channel bytes; no indexed palette |

For indexed modes, classify a symbol by nearest calibrated reference, with normalized
distance at most 0.35 and nearest/runner-up margin at least 0.15 (0.10 for 16 colors).
For RGB888, black/white anchors normalize each channel, then the receiver rounds to an
8-bit channel value. A single altered byte fails frame CRC; RGB888 is therefore suitable
only for a sufficiently lossless pixel path. Sender Canvas readback compares local colors
with these protocol values; it cannot measure pixels after VNC compression or scaling.

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
