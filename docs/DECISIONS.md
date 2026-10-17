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


## 2026-10-17：增加 512×512 与 1028×512

按用户明确指定保留 1028 这一宽度。继续使用固定的 profile ID 4/5，无需先解码配置即可定位；
矩形的水平、垂直 finder 间距分别由 W+17、H+17 定义，行长为 W，调色板在 H+20。
512×512 帧有效载荷 65468 字节，可放入原有 16 位长度字段。1028×512 为 131516 字节，
因此仅 profile 5 把头部 byte 34–35 定义为载荷长度高 16 位，低位仍在 byte 32–33。
两字节均在 CRC 保护范围内；旧 profile 的 byte 34–35 仍必须为零。Rust 头部载荷长度改为 u32。

发送端与接收端都限制图像宽高至 8192 像素且总像素不超过 32 Mi；网页按 profile 自动限制物理 scale。
1028×512 在 2 像素/符号时画布为 2120×1088，适合先做桌面/VNC 实测。
矩阵面积增大使单帧绘制、采集和 VNC 更新成本上升；实际有效吞吐不能仅由载荷容量推断。

## 2026-10-18：大矩阵画布可见性提示

实测记录中的 3653632 字节/224 块对应 256×256；1614499 字节/25 块对应 512×512，
尚无 1028×512 的连续接收日志。旧发送页的 `main` 最大宽度为 1100 像素且居中，
使宽 2120 像素的 1028×512（每符号 2 像素）画布从容器左边向右溢出，右侧定位标记可能离开窗口。
因此移除该居中宽度限制，按实际画布矩形与浏览器可见区域提示裁切，并显示画布像素尺寸。
页面仍保持整数符号像素，不用 CSS 缩放冒险改变采样质量。


## 2026-10-18：增加 512×256

按用户指定增加第六个固定 profile，保留既有 ID 1–5 不变。新 profile 为 ID 6，
帧长度 32768 字节、数据块有效载荷 32700 字节、画布 544×288 逻辑符号。
其载荷可放入原有 16 位长度字段，byte 34–35 仍必须为零；
仅 profile 5 使用这两字节作为高位。定位、定时线和调色板沿用已测试的矩形通用路径。


## 2026-10-18：增加 256×128

按用户指定增加第七个固定 profile；旧 ID 1–6 和数据帧语义不变。profile 7 的帧长为
8192 字节、有效载荷 8124 字节、逻辑画布 288×160，仍使用 16 位载荷长度字段。
矩形定位/采样沿用 profile 5/6 的通用路径，独立测试向量明确新 ID 与画布尺寸。

## 2026-10-18：精简发送页状态文案

发送页只在播放后显示轮次和帧进度；画面超出当前窗口时才显示缩小像素或放大窗口的提示。
移除画布像素尺寸、未播放帧占位、重复操作说明，以及目标/本地 FPS/间隔状态行。
目标帧率选择器保留，用于设置发送端播放节奏；它不代表 VNC 实际接收帧率。

## 2026-10-18：新增颜色模式与本地颜色自检

用户要求 2、4、8、16 色及每符号 RGB 24 位。4 色继续使用逐字节不变的 VMT1 版本 1；
其余模式使用版本 2，在定位后读取固定黑白引导条确定每符号位数，再读取模式对应的参考色块。
头部 byte 7 固定颜色模式，32–35 为 32 位载荷长度；几何 profile ID 1–7 不变。
接收器同时校验引导条、头部、CRC、元数据块容量和传输 ID，避免跨模式拼接。
RGB888 直接承载三通道原字节，吞吐潜力高，但对 VNC 有损变化极其敏感；
保持 CRC 拒绝，不以颜色近似值冒充已验证数据。
发送端「颜色自检」通过 Canvas `getImageData` 对比固定参考色块和部分 RGB 数据符号，
展示预设、读回颜色和最大差值。此检查仅覆盖本地 Canvas，无法代表 VNC 接收图像。

接收 CLI 的 PNG 与实时 Portal/PipeWire 路径共用 `decode_bitmap`；首次有效元数据锁定矩阵尺寸和颜色模式。
终端及结果 JSON 报告自动识别的格式，方便核对实际发送选择。PipeWire BGRx 裁切测试覆盖
4 色旧版和 2/8/16/RGB 新版的元数据、数据帧。

## 2026-10-18：发送页隐藏成功诊断

按用户要求移除已停止状态文案和 SHA-256 展示面板；后台 SHA-256 计算及接收端最终校验保持原有逻辑。
本地 Canvas 颜色自检仍运行，但一致时不显示面板，只在发现色差或读取失败时展示诊断。
