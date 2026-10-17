# Portal / PipeWire 采集：构建与实际验证

本功能已实现，需通过 `live-capture` feature 启用。2026-10-17 已通过本机编译和组件测试；
当前执行环境访问桌面 D-Bus 被拒绝；用户已在普通 GNOME 终端完成首张截图，2560×1388 的
本机浏览器画面已通过定位、颜色分类和 CRC。连续采集已恢复 40/40 块、158173 字节文件，SHA-256 一致且接收器自动结束；撤销授权及真实 VNC 完整恢复仍待实测。
Phase 2 自动循环播放器已实现；目标 Firefox 115 ESR 和完整 VNC MVP 尚未验收。

## 构建

```bash
pkg-config --modversion libpipewire-0.3 libspa-0.2
cargo build --offline --locked --release --manifest-path receiver/Cargo.toml --features live-capture
```

当前桥接用 GCC/cc 编译系统头文件，不使用 bindgen，因此不需要 clang 或 libclang。
使用已缓存且锁定的 zbus、futures-lite、async-channel；首次准备依赖的其他机器先运行
`cargo fetch --locked --manifest-path receiver/Cargo.toml`。普通离线构建完全不依赖系统 PipeWire 开发包。

## 第一步：保存一张图

在用户自己的 GNOME Wayland 终端运行（不用 sudo）：

```bash
receiver/target/release/vmtransfer snapshot --output target/portal-first.png --source any
```

选择 VNC 窗口或屏幕；首张帧保存为 PNG，随后关闭流和授权会话。支持 `--source window|monitor|any`，
默认 any；窗口支持取决于后端。现有输出路径拒绝覆盖。选择后 15 秒内没有首帧会返回超时。

打开 PNG 检查矩阵、静区和色块是否完整。可把图像放入离线清单，确认定位/CRC：

```bash
python3 - <<'PY'
from pathlib import Path
import json
Path('target/snapshot-images.json').write_text(json.dumps([str(Path('target/portal-first.png').resolve())]))
PY
receiver/target/release/vmtransfer decode --manifest target/snapshot-images.json --output-dir target/snapshot-received
```

单张普通元数据/数据帧通常返回退出码 2（尚未收齐），这是预期结果。只有空文件的元数据
可单帧完成；对于普通文件，需完整序列才能验证 SHA。通过日志/最终 JSON 查看 CRC、定位、颜色失败数。

## 第二步：连续接收

```bash
receiver/target/release/vmtransfer receive --output-dir target/live-received --source window
```

也可使用显式构建命令（离线测试可能把默认 release 二进制重建为不带采集的版本）：

```bash
cargo run --offline --locked --release --manifest-path receiver/Cargo.toml --features live-capture -- \
  receive --output-dir target/live-received --source window
```

刷新 sender/index.html，选择文件并等待准备结束，保持 5 FPS 起步，点击「开始播放」；
在 GNOME 授权框选择 VNC 窗口。接收器可以晚加入，会在后续元数据初始化并跨循环补齐缺块。
接收器成功退出后手动停止发送端。若换了文件，需启动新的接收会话，避免把不同 ID 混在一起。

源图像按最新帧进入同一个解码器；每秒有图像到达时输出接收进度，最终输出 JSON。
终端显示的唯一块率是本次观测值；绘制速率、捕获率、重复率和最终 goodput 要分别解释。
成功后立即停止消费并释放采集；临时文件遵循原有长度/SHA 校验和禁止覆盖规则。
用户停止共享或 PipeWire 断开会报告错误并清理未完成输出。强制杀进程仍可能留下 `.part`。
静止窗口/暂停发送可无限等待后续更新，不能以“没有新画面”直接断定连接中断。

## 当前支持与边界

* Portal CreateSession → SelectSources → Start → OpenPipeWireRemote；先订阅 Response，防止早到信号丢失。
* 单个窗口/屏幕，不保存恢复 token；支持的源类型从 Portal 属性查询。选择隐藏或元数据鼠标模式。
* PipeWire 只连接授权 FD；可用时优先 stream serial，否则使用当前会话的 node ID。
* 支持映射到 CPU 的 BGRA/BGRx/RGBA/RGBx/RGB/BGR。每个 buffer 独立处理尺寸、stride、offset、crop。
* 当前明确拒绝负 stride、DMA-BUF-only、带 modifier 或未知格式，不把这些内容误当 RGB。
* 队列只保留一张最新图像，`buffers` 是转换的帧数，`overwritten` 是 Rust 队列中覆盖的旧帧数，
  不等于 VNC 网络丢帧数，也不包含 PipeWire 队列中被跳过的缓冲。
* 只有本功能使用本地 D-Bus/PipeWire IPC；远端文件仍只经 VNC 视觉通道进入接收器。

## 失败定位

* `cannot connect to desktop session bus`：确认在真实桌面用户终端运行，环境中有正确的
  DBUS_SESSION_BUS_ADDRESS。当前受限执行环境的 `busctl --user status org.freedesktop.portal.Desktop`
  返回 `Operation not permitted`，需要在普通桌面终端执行命令，无需重新安装开发包。
* `ScreenCast portal unavailable`：检查用户级 xdg-desktop-portal 和 GNOME 后端。
* `capture selection cancelled`：用户取消；不会自动重新弹框。
* 格式不支持/首帧超时：记录错误、GNOME/PipeWire 版本和所选源，分别试窗口及屏幕模式定位。

测试命令与实际限制记录在 VALIDATION.md；实际 VNC 速率测试按 BENCHMARK.md 执行。
