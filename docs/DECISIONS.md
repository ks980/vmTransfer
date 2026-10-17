# Design decisions

## 2026-10-16 — Phase 0 / Phase 1

* Implement the offline codec first. Portal/PipeWire and a full GUI remain Phase 3.
* Treat 128x128 as the binary encoding area; external finders and calibration make
  the full profile 160x160. This leaves a fixed 4028-byte payload per data frame.
* Use a predefined geometry instead of a configurable bootstrap header. A decoder
  can locate and sample an image without trusting any decoded configuration.
* Repeat metadata before every 64 data blocks as well as each loop, limiting late
  join latency. The transfer ID and all encoded frames stay stable across loops.
* Cap v1 files at 1 GiB and use disk-backed random-access block storage plus a
  bounded bitmap. Oversized declarations are rejected before allocating storage.
* Vendor js-sha256 0.11.1 (MIT) for incremental browser hashing. Web Crypto digest
  requires a whole input buffer; a dependency avoids writing our own SHA-256.
  Node tests explicitly exercise the browser implementation against Node crypto.
* Keep a minimal browser inspector and offline CLI in Phase 1 because they make the
  complete codec observable. A timed sender player and production UI are Phase 2.
* Atomic output uses Linux renameat2(RENAME_NOREPLACE), through rustix, to avoid
  both overwrites and a check-then-rename race. No fallback may overwrite a file.
* Export traits/JSDoc contracts for future capture/player modules, but do not ship
  fake implementations claiming live capture or VNC throughput.

References: [Web Crypto digest](https://developer.mozilla.org/en-US/docs/Web/API/SubtleCrypto/digest),
[js-sha256 v0.11.1](https://github.com/emn178/js-sha256/tree/v0.11.1),
[ScreenCast portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html),
[ashpd](https://docs.rs/ashpd/latest/ashpd/desktop/screencast/index.html).

## 2026-10-17 — optional native capture

* User-installed PipeWire development headers are now available (1.4.2, SPA 0.2).
* Cargo's configured proxy is unreachable, and ashpd/pipewire-rs are not cached.
  Use cached zbus 5.19.0 for the documented Portal D-Bus API and a narrow C bridge
  against installed PipeWire headers. This builds offline with GCC and does not
  need libclang/bindgen. Do not silently fetch different/unlocked dependencies.
* Gate capture and all added dependencies behind `live-capture`; default/offline
  compilation never invokes pkg-config or the C compiler.
* Use only OpenPipeWireRemote's authorized FD. Prefer a portal-provided stream
  serial if available; otherwise retain its node ID for this session only.
* A one-frame latest slot bounds decoder backlog. Callbacks copy mapped RGB data
  and immediately requeue buffers; protocol decoding and storage stay outside.
* Deadline only the initial frame (15s). Static windows and paused senders may
  legitimately emit no further updates. A Portal Closed watcher wakes blocked reads.
* Stop/join the PipeWire thread before dropping callback state, cancel/join the
  Portal watcher, and close the session. No permission persistence or restore tokens.
* This execution environment cannot connect to the desktop D-Bus (EPERM).
  Compiled code and synthetic tests are not claimed as live desktop/VNC acceptance.

## 2026-10-17 — Phase 2 automatic sender

* Implement the timed player independently of browser UI, sharing cycleFrames and
  the exact binary encoder. There is no receiver feedback or missing-block input.
* Use performance.now deadlines and rounded-up browser timers, not rAF frequency.
  After late callbacks/slow reads, present one frame and wait a full new period.
  Never catch up in bursts or skip logical blocks. Report observed local FPS only.
* Keep at most one active encoded read-ahead per run and advance only after painting.
  AbortController plus generation tokens suppress cancelled/stale read completions.
* Pause keeps the visible image and next unshown frame; stop clears the image and
  resets the cursor. Both retain the prepared ID/hash. Only preparation creates an ID.
* Playback errors clear the image and are terminal for that player. Reselect the file
  to prepare a fresh transfer. No silent retry that might mix changing input content.
* The normal sender UI now plays automatically and has no manual block-index input.
  Existing static browser smoke fixtures remain available for codec diagnostics.
* Validate rates and lifecycle with a fake monotonic clock; additionally feed actual
  player-generated PNGs (with omissions/duplicates over three loops) to the Rust CLI.
  Passing simulated-clock tests is not evidence of actual VNC or Firefox 115 throughput.


## 2026-10-17：增加预定义矩阵 profile 2/3

按用户要求提供 128×128、160×160、256×256 三档数据矩阵。保留 VMT1 version 1 的 64 字节头部和
CRC/SHA 语义；原 profile 1 保持逐字节兼容，新增 profile 2/3 明确使用头部 byte 6 区分。
帧长为 N²/4，有效载荷为 N²/4−68；定位和白色边框随 N 扩展，外框边长 N+32。
接收器只尝试三个预定义几何布局，确认 CRC 和头部 profile，避免先解码配置才能定位的循环依赖。
元数据 block_size 必须匹配 profile；会话按 block_size 计算偏移并拒绝跨 profile 数据。

矩阵属于一次传输的固定属性，播放/暂停时禁用选择；停止后切换会重新哈希准备并生成新 ID，
页面提示新建接收会话。物理符号像素仍可独立调整。大矩阵的目标 VNC 稳定性需要实际测量。
组合失真测试中 3 像素/符号经双线性缩放、旋转、颜色扰动时，大矩阵曾出现调色板拒绝；
保持原阈值，按原组合测试的 4 像素/符号验证，并保留该限制记录。
