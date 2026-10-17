# 开发、部署和实际采集前的环境准备

## 当前检查结果

2026-10-16：Debian GNU/Linux 13.7，GNOME Wayland 会话；Rust/Cargo 1.95.0，Node 20.19.2，
Python 3.13.5，Firefox 157.0.1。已安装 PipeWire 1.4.2、xdg-desktop-portal 1.20.3、
xdg-desktop-portal-gnome 48.0。最初缺少 libpipewire-0.3-dev。

2026-10-17 更新：用户已安装开发依赖，`pkg-config --modversion libpipewire-0.3 libspa-0.2`
分别返回 `1.4.2`、`0.2`，GCC 和 libclang 均可用。实际适配层使用 GCC 编译 C ABI 桥接，
无需独立 clang 命令或 libclang/bindgen。当前执行环境访问用户会话总线返回 `Operation not permitted`。

检查命令：

```bash
rustc --version
cargo --version
node --version
python3 --version
firefox --version
printenv XDG_SESSION_TYPE WAYLAND_DISPLAY
pkg-config --modversion libpipewire-0.3
dpkg-query -W xdg-desktop-portal xdg-desktop-portal-gnome pipewire libpipewire-0.3-dev
```

## Windows 7 / Firefox 115 ESR

复制完整 sender 目录后直接打开 index.html。仅需要 File/Blob、FileReader、TypedArray、
TextEncoder/TextDecoder、Canvas 2D、AbortController、Promise/async、crypto.getRandomValues。
使用普通脚本标签，无 fetch、ES module、WebGPU、WebCodecs 或 getDisplayMedia 要求。
SHA 使用随附 JS 增量实现，不依赖 Web Crypto digest 的整文件缓冲。

手工验证：选择空文件、随机二进制和 Unicode 文件名；检查准备进度、取消、大文件分块读取、
循环计数、比例修改和重新选文件。保持页面前台，避免后台标签页限速。
播放器已实现；需实机分别测试六档速率、暂停续播、停止重播和换文件时的异步取消。
暂停保留画面，停止清空画面；两者均保留同一传输 ID。首次完整接收从 5 FPS 开始。

## 浏览器 Canvas 冒烟测试

`tests/browser-smoke.html` 会在真实 Canvas 上画固定内容，显示 PASS/FAIL。打开无查询参数版本
得到元数据；打开 `?data=1` 得到数据帧。恢复结果应为 0..255 的全部字节。

可在支持 headless 的开发环境执行（使用独立 profile）：

```bash
mkdir -p target/firefox-smoke-profile
firefox --headless --no-remote --profile "$PWD/target/firefox-smoke-profile" \
  --screenshot "$PWD/target/browser-meta.png" --window-size 800,800 \
  "file://$PWD/tests/browser-smoke.html"
firefox --headless --no-remote --profile "$PWD/target/firefox-smoke-profile" \
  --screenshot "$PWD/target/browser-data.png" --window-size 800,800 \
  "file://$PWD/tests/browser-smoke.html?data=1"
```

把这两张 PNG 的路径按顺序放入 CLI 清单，期望输出 `browser-smoke.bin`，再与
`protocol/vectors/all-bytes/input.bin` 比较。当前环境第一次 headless 命令退出 139，
未得到可验证截图；不能据此声称真实浏览器通过。Node VM 浏览器全局路径测试已通过，
但它不替代 DOM/FileReader/Firefox 115 实机测试。

## Debian Phase 3 依赖与授权

Phase 1 不需要 PipeWire 系统开发包。可选 live-capture 构建需要：

```bash
sudo apt install build-essential pkg-config libpipewire-0.3-dev \
  pipewire xdg-desktop-portal xdg-desktop-portal-gnome
```

系统安装由用户执行。Cargo 锁定 zbus 5.19.0、futures-lite 2.6.1、async-channel 2.5.0，
通过 `live-capture` feature 启用。当前 Cargo 网络代理不可用且 ashpd/pipewire Rust 包未缓存，
因此采用 zbus 调用标准 Portal，加小型 C ABI 适配层调用本机 PipeWire，而非引入无法构建的依赖。
参见 LIVE_CAPTURE.md 的命令与验证边界。

Portal 调用必须运行在用户的桌面会话中。用户通过 GNOME 选择窗口或屏幕；能否单独选择窗口
取决于 Portal 后端能力。拒绝/取消授权应返回可理解错误，不得切换到 X11 截图或静默读其他应用。
PipeWire 适配涉及系统头文件、协商格式和异步缓冲区生命周期，必须先验证。
优先协商可映射的 RGB 缓冲；不支持的 DMA-BUF/像素格式要明确拒绝或在适配层转换。
