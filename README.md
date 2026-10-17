# VMT — 经 VNC 视觉通道单向传输一个文件

发送端目标：Windows 7 / Firefox 115 ESR，静态 HTML + vanilla JS + Canvas。
接收端目标：Debian GNOME Wayland 原生 Rust 应用。

**当前完成 Phase 0、Phase 1：协议、JS 编码器、Rust PNG/位图解码器、跨循环收集、
SHA-256 验证和原子保存。** 另已实现可选的 Portal/PipeWire 授权采集和实时接收命令，
编译及组件测试通过，用户桌面终端已连续接收并恢复 158173 字节文件，40/40 块及 SHA-256 验证通过。
Phase 2 自动循环发送器已实现并通过离线端到端测试；Firefox 115 ESR/Windows 7 及实际 VNC 连续接收仍待实测。
完整原生 GUI 尚未实现，实际 VNC 吞吐量尚未测量。

发送页面提供 **128×128、160×160、256×256、512×512、1028×512、512×256、256×128** 数据矩阵，默认 128×128：

| 数据矩阵 | 每帧有效载荷 | 含定位边框的逻辑尺寸 |
|---|---:|---|
| 128×128 | 4028 字节 | 160×160 |
| 160×160 | 6332 字节 | 192×192 |
| 256×256 | 16316 字节 | 288×288 |
| 512×512 | 65468 字节 | 544×544 |
| 1028×512 | 131516 字节 | 1060×544 |
| 512×256 | 32700 字节 | 544×288 |
| 256×128 | 8124 字节 | 288×160 |

发送页面还可选择 **2 色、4 色、8 色、16 色、RGB 24 位**。上表载荷适用于默认 4 色；
其他模式的载荷为 `矩阵宽×高×每符号位数÷8−68` 字节，其中 RGB 为每符号 24 位。
2/8/16 色和 RGB 使用版本 2 帧；旧 4 色帧仍按版本 1 解码。RGB 对像素变化很敏感，
实际 VNC 可用性需在目标链路测量。页面仅在本地 Canvas 颜色与预设不一致时显示对比色块及最大色差；
它无法检测 VNC 压缩后的画面。

刷新发送页面后选择“数据矩阵”和“颜色”，再选文件开始播放。停止后切换会重新准备文件、生成新传输 ID，
需启动新的接收会话。接收器从画面自动识别七档矩阵与颜色模式，无需额外参数；
终端会显示识别结果，最终 JSON 的 `visual_format` 也包含矩阵宽高和颜色模式。
更新后用带 `live-capture` 的 cargo 命令重新构建。
“每符号像素”独立控制显示比例；例如 1028×512 在 2 像素/符号时画布为 2120×1088 像素。
大矩阵可选像素上限会自动限制到接收器支持的 8192 像素单边、32 Mi 像素总量；请让整个画布在 VNC 窗口内可见。
页面只在画布超出当前浏览器窗口时提示。测试 1028×512 时先选每符号 2 像素，
此时画布为 2120×1088；如仍被浏览器工具栏或 VNC 窗口裁切，可让浏览器全屏或扩大显示区域。
对 1614499 字节文件，4 色时 1028×512 为 13 个数据块，512×256 为 50 个，
256×128 为 199 个；换颜色模式后块数会变化。

文件内容只进入彩色图像。应用没有网络文件通道、剪贴板、ACK、压缩或 FEC。
PNG 压缩仅用于测试图像容器，不是待传文件的压缩功能。

## 快速验证

在仓库根目录运行。已验证环境：Rust/Cargo 1.95.0、Node.js 20.19.2、Python 3.13.5。
Node/Python 仅用于开发和测试，远端 Windows 不需要安装它们。

```bash
python3 tests/generate-vectors.py --check
node tests/js/codec.test.cjs
node tests/js/player.test.cjs
node tests/js/ui.test.cjs
cargo test --offline --locked --manifest-path receiver/Cargo.toml
node tests/e2e.cjs
cargo fmt --manifest-path receiver/Cargo.toml --check
cargo clippy --offline --locked --manifest-path receiver/Cargo.toml --all-targets -- -D warnings
cargo doc --offline --locked --manifest-path receiver/Cargo.toml --no-deps
```

Rust 依赖已由 `receiver/Cargo.lock` 锁定。`--offline` 要求依赖在本机缓存中；
新开发机可先运行 `cargo fetch --locked --manifest-path receiver/Cargo.toml`。
应用运行及 Windows 页面加载不需要网络，也没有 npm 安装步骤。

`tests/e2e.cjs` 构建 release CLI，生成 JS PNG，注入图像/帧序列扰动，再让 Rust
从图片恢复文件。每次运行的 PNG、清单和结果保存在 `target/e2e-*`；最近结果为
`target/latest-e2e-results.json`。测试固定向量位于 `protocol/vectors/`，重新生成需显式运行
`python3 tests/generate-vectors.py`。修改协议时须评审向量变化，不能只重写预期结果。

## 手动运行离线接收器

以下清单故意包含乱序、重复和下一轮补块，清单内容只有图片路径：

```bash
mkdir -p target
python3 - <<'PY'
import json
from pathlib import Path
root = Path.cwd()
frames = [str(root / 'protocol/vectors/random' / f'frame-{i:03}.png')
          for i in [0, 4, 2, 2, 0, 1, 3]]
Path('target/demo-images.json').write_text(json.dumps(frames))
PY
cargo run --offline --locked --manifest-path receiver/Cargo.toml -- \
  decode --manifest target/demo-images.json --output-dir target/demo-received
cmp protocol/vectors/random/input.bin target/demo-received/random.bin
sha256sum protocol/vectors/random/input.bin target/demo-received/random.bin
```

再次运行请换一个输出目录；程序拒绝覆盖已有文件。退出码：`0` 已验证保存，
`2` 图片用尽但尚未收齐，`1` 致命错误。进度事件写 stderr，最终 JSON 写 stdout。
无效 CRC/定位/颜色帧被丢弃；矛盾数据、最终哈希错误及存储错误使会话失败。

## Windows 7 页面部署

完整复制 `sender/` 到远端已有可用目录，在 Firefox 115 ESR 中打开 `sender/index.html`。
所有脚本和 SHA 依赖随目录提供，支持 `file://`，不使用 CDN、模块下载或构建步骤。
选择单个文件，等待分块 SHA-256 完成，然后点击「开始播放」。默认 5 FPS、每符号 4 像素；
可选择 5/10/15/20/25/30 FPS 和整数符号尺寸。首帧及每组 64 数据块前发送元数据，末帧后
自动从头循环，直到手动停止。文件名、大小、帧序号、循环次数和准备进度可见。
SHA-256 仍在后台计算和验证，但页面不再展示摘要文本。

「暂停」保留画面，点击「继续播放」接着发送；「停止」清空画面，下一次从头播放。
暂停/停止/重播不更换传输 ID，重新选择文件才建立新传输。更换文件会取消旧准备和旧播放任务。
接收成功不会自动停止发送器，需手动点击停止。保持页面前台和完整静区/三个定位标记可见；
如果画面放不下，调小符号像素。目标 FPS 不代表 VNC 实际接收 FPS。

复制更新的 sender 目录或刷新已有页面后即可使用，无构建步骤。实际 Firefox 115 ESR/Windows 7
及目标 VNC 链路仍需验证。调度与接口见 `sender/player/player.js` 和 `contracts.js`。

## Portal / PipeWire 实时采集

在普通 GNOME Wayland 桌面终端运行，使用 GCC 即可：

```bash
sudo apt install build-essential pkg-config libpipewire-0.3-dev
cargo build --offline --locked --release --manifest-path receiver/Cargo.toml --features live-capture
receiver/target/release/vmtransfer snapshot --output target/portal-first.png --source any
```

GNOME 会显示共享选择框，选择 VNC 窗口或屏幕；保存一张 PNG 后释放采集资源。`--source`
可选 `window`、`monitor`、`any`。输出路径必须尚不存在。确认截图中矩阵完整可见后，可运行：

```bash
receiver/target/release/vmtransfer receive --output-dir target/live-received --source window
```

接收器复用已有图像解码、跨循环补块、CRC/SHA 校验和原子保存，成功后关闭采集。
首张视频帧等待上限 15 秒；之后允许暂停/静止窗口不产生更新，用户停止共享会终止等待。
默认构建不启用采集，也不依赖 PipeWire 开发包。当前执行环境无法访问用户会话总线，
用户已在桌面终端完成首张截图及连续实时接收，文件长度、SHA-256 和成功后自动结束均验证通过。
详见 [采集验证步骤](docs/LIVE_CAPTURE.md)。

若运行离线测试后需要连续接收，可显式启用采集 feature 启动，避免使用被默认离线构建替换的二进制：

```bash
cargo run --offline --locked --release --manifest-path receiver/Cargo.toml --features live-capture -- \
  receive --output-dir target/live-received --source window
```

## 设计与后续填充入口

* [协议、视觉布局、CRC 范围](protocol/SPEC.md)
* [目录、关键类型、函数签名及后续填充位置](docs/ARCHITECTURE.md)
* [实现阶段与验收门槛](docs/IMPLEMENTATION_PLAN.md)
* [技术决策及理由](docs/DECISIONS.md)
* [Debian Wayland 与 Windows 环境、浏览器验证](docs/SETUP.md)
* [测试结果与已知限制](docs/VALIDATION.md)
* [离线重复测量及未来 VNC 基准流程](docs/BENCHMARK.md)

限制：v1 文件上限 1 GiB、单文件、无重启续传、无遮挡的固定布局；支持平移、横纵缩放和
90° 倍数旋转，不处理相机透视。成功只保存一次，失败不发布文件。正常退出清理临时文件；
强制杀进程/断电可能留下 `.vmt-*.part`，它不是已验证输出，也不会自动恢复。
