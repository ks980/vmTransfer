# Phase 0 / Phase 1 验收记录

日期：2026-10-16。工作目录：`/home/kris/demo/vmTransfer`。
环境见 SETUP.md。以下是实际执行结果；实时 VNC 和目标浏览器实测不在通过项中。

## 已执行命令与结果

| 命令 | 最终结果 |
|---|---|
| `python3 tests/generate-vectors.py` | 生成 54 个独立参考文件，9 类输入，共 22 个二进制帧及对应 PNG |
| `python3 tests/generate-vectors.py --check` | 54 个文件逐字节一致 |
| `node tests/js/codec.test.cjs` | 17 项通过，0 失败 |
| `cargo test --offline --locked --manifest-path receiver/Cargo.toml` | 18 项 Rust 集成测试通过，0 失败 |
| `node tests/e2e.cjs` | 28 个 JS → PNG → Rust 场景通过，包含预期不完整退出 |
| `cargo fmt --manifest-path receiver/Cargo.toml --check` | 通过 |
| `cargo clippy --offline --locked --manifest-path receiver/Cargo.toml --all-targets -- -D warnings` | 通过，无警告 |
| `cargo doc --offline --locked --manifest-path receiver/Cargo.toml --no-deps` | 成功生成 Rust API 文档 |

开发过程中还执行了 `cargo fmt --manifest-path receiver/Cargo.toml`，以及首次离线构建
`cargo test --offline --manifest-path receiver/Cargo.toml`，生成依赖锁文件。
`node --test tests/js/*.test.cjs` 也通过；该环境汇总为单个文件，故最终使用上面的直接运行命令
显示全部 17 项明细。初次 clippy 发现一处 needless_range_loop，已修正并重新通过。

Rustdoc 输出为 `receiver/target/doc/vmtransfer/index.html`。最后一次 e2e 的完整图片、清单与结果
保存在 `target/e2e-tCSyBE/`；`target/latest-e2e-results.json` 指向同一次结果内容。
这些是本地生成物；新环境运行相同命令会生成新的目录。

## 证据与覆盖范围

* 共享向量由独立 Python `struct` / `hashlib` / `zlib.crc32` 实现生成，不导入 JS/Rust 代码。
  JS 对比完整帧字节；Rust 从 PNG 定位、采样并对比解析后的头部和载荷，最后比较恢复文件。
* 覆盖空文件、零字节、全部 256 种字节、4027/4028/4029 字节边界、随机数据、Unicode 文件名。
  JS 额外验证 BOM/组合字符、带偏移的 Buffer/Uint8Array，以及无 CommonJS/Node crypto 的浏览器全局加载路径。
* 28 个 e2e 场景覆盖缩放、双线性/最近邻、背景偏移、横纵不同倍率、旋转、色偏/噪声和组合扰动。
* `repeated-loops-with-loss`：数据先于元数据、首轮缺块、重复、局部混合、坏 CRC、白色非法符号、异会话、
  元数据重复及后续补齐。结果：4/4 块，2 个 CRC 拒绝，1 个调色板拒绝，2 个重复，1 个异会话拒绝，
  最终文件逐字节一致。
* `incomplete`：只收 1/4 块，退出码 2；无发布文件，也无正常退出后遗留的临时文件。
* 完成序列后附加不存在的 PNG 路径，接收器仍成功退出，证明成功后没有继续读取后续帧。
* Rust 验证同 ID 元数据/重复块冲突、最终错误 SHA、空文件错误 SHA、文件写入失败、目标已存在和
  发布时目标出现的竞态；全部不发布损坏输出、不覆盖已有文件。
* 按 u64 打包的到达位图使用 66 块逆序输入测试，覆盖 63/64 边界和重复；1 GiB 元数据边界及超限拒绝已测。
* 资源检查覆盖无效 stride、图像尺寸、畸形 PNG、帧边界、CRC 正确但非法字段，以及 1000 组随机 CRC-valid 头部。

README 的手工示例也已运行：

```bash
cargo run --offline --locked --manifest-path receiver/Cargo.toml -- \
  decode --manifest target/demo-images.json --output-dir target/demo-received
cmp protocol/vectors/random/input.bin target/demo-received/random.bin
sha256sum protocol/vectors/random/input.bin target/demo-received/random.bin
```

`cmp` 退出 0；两者 SHA-256 均为：

```text
3b5fb7f4b7bdac07676db0e7d03243694e5f64b767e041acdb532ca239e07693
```

## 未通过或尚未执行的环境验证

已尝试：

```bash
firefox --headless --no-remote \
  --profile /home/kris/demo/vmTransfer/target/firefox-smoke-profile \
  --screenshot /home/kris/demo/vmTransfer/target/browser-meta.png \
  --window-size 800,800 file:///home/kris/demo/vmTransfer/tests/browser-smoke.html
```

本机 Firefox 157.0.1 输出 headless 提示后退出 139，没有可验收截图。尚未验证实际 DOM/FileReader
和 Canvas 截图链路，也未取得 Windows 7 / Firefox 115 ESR 的运行结果。保留 browser-smoke.html 供复现。

开发 shell 的 curl 由于代理连接/DNS 问题无法下载 js-sha256；已通过可用网页读取工具取得固定 tag 的
完整源码与 MIT 许可证，保留来源和本地校验和，浏览器纯 JS 哈希与独立 Node 哈希对照通过。

## Phase 0/1 完成时的下一步阻碍（历史记录）

自动循环播放器、Portal/PipeWire 采集和原生进度 UI 只有明确接口，尚未实现。
本机缺少 `libpipewire-0.3-dev`；还需验证 ashpd/pipewire 依赖和授权流，准备 Windows 7 / Firefox 115 ESR
及实际 VNC 会话。只有完成真实链路后才能宣称 MVP 完成或报告 VNC 吞吐量。

## 2026-10-17：可选 Portal/PipeWire 实现与验证

用户已安装开发包。`pkg-config --modversion libpipewire-0.3 libspa-0.2` 返回 `1.4.2`、`0.2`；
系统 GCC/cc 可用。由于 Cargo 的 localhost:3128 代理不可连接，未缓存的 ashpd/pipewire-rs 无法下载。
实际实现采用已缓存 zbus 和 GCC 编译的小型 C ABI 桥接，不需要 clang/libclang/bindgen。

实际执行：

| 命令 | 结果 |
|---|---|
| `cargo check --offline --locked --manifest-path receiver/Cargo.toml --features live-capture` | 编译通过 |
| `cargo test --offline --locked --manifest-path receiver/Cargo.toml --all-features` | 8 项采集单元测试 + 19 项集成测试，全部通过 |
| `cargo clippy --offline --locked --manifest-path receiver/Cargo.toml --all-targets --all-features -- -D warnings` | 通过，无警告 |
| `cargo doc --offline --locked --manifest-path receiver/Cargo.toml --all-features --no-deps` | 通过 |
| `node tests/e2e.cjs` | 28 个离线端到端场景全部通过；产物 `target/e2e-jp7ypJ/` |
| `cargo build --offline --locked --release --manifest-path receiver/Cargo.toml --features live-capture` | 生成可运行的原生采集 CLI |

新增测试覆盖：六种 packed RGB 格式、带 padding 的 stride、chunk offset、crop、越界及负 stride 拒绝；
真实 golden PNG 经模拟映射 BGRx 缓冲和裁剪后进入统一 decoder；最新帧覆盖计数、初始超时、停止/失败唤醒；
Portal response 取消/失败区别和 token 唯一性；诊断 PNG 往返和禁止覆盖。
这些组件测试没有模拟成功的桌面授权，也不代替真实 PipeWire 帧验证。

运行 `receiver/target/debug/vmtransfer snapshot --output target/portal-first.png --source any`
返回 Portal 不可用，未生成截图。进一步执行：

```bash
busctl --user status org.freedesktop.portal.Desktop
```

返回 `Failed to connect to user scope bus via local transport: Operation not permitted`。
因此当前执行环境无法启动桌面共享选择框。代码已提供 `snapshot`、`receive`，但真实授权、
视频格式协商、采集成功/撤销流程以及 VNC 恢复和速率仍待用户桌面终端实测。
具体命令见 LIVE_CAPTURE.md；Phase 3 不标记为已验收。

### 用户桌面终端的首张采集图验证

用户在 GNOME 终端执行 snapshot 后生成 `target/portal-first.png`，图像尺寸 2560×1388。
已检查图像，内容为本机 Firefox 的 `sender/index.html` 页面，编码矩阵、三个定位标记和参考色块完整可见。
这张图是本机浏览器采集验证，不作为实际 VNC 传输的证据。

使用仅包含该图片路径的清单执行：

```bash
receiver/target/release/vmtransfer decode \
  --manifest target/capture-check-_m7p3f6s/images.json \
  --output-dir target/capture-check-_m7p3f6s/received
```

结果：退出码 2，`WaitingMetadata`；images=1、valid_frames=1、waiting_metadata=1；
CRC/定位/颜色/格式失败均为 0。说明真实采集图像已成功定位、采样并通过完整帧 CRC。
该图是数据帧，尚无会话元数据，因此正确拒绝存储，不发布文件。
下一步仍需元数据与连续数据帧的实时接收，以及实际 VNC 和完整文件 SHA 验证。

## 2026-10-17：Phase 2 自动循环发送器

已实现 sender/player/player.js 和页面播放控制，使用单调期限、有限预读、AbortController 和代次保护。
UI 支持准备取消、开始/暂停/继续/停止、六档 FPS、整数符号尺寸、帧/循环进度、本地绘制 FPS 和间隔、后台提示。
正常页面不再提供手工数据块索引输入。没有协议或固定向量变更。

实际执行：

| 命令 | 结果 |
|---|---|
| `node tests/js/codec.test.cjs` | 17 项通过 |
| `node tests/js/player.test.cjs` | 13 项通过 |
| `node tests/js/ui.test.cjs` | 6 项通过 |
| `python3 tests/generate-vectors.py --check` | 54 个向量文件一致 |
| `node tests/e2e.cjs` | 30 个场景通过，产物 `target/e2e-hwZ2Pw/` |

播放器测试覆盖每组 64 块前的元数据、空文件、三轮以上循环、六档时钟调度、早到/迟到定时器、
缓慢文件读取、暂停/继续不跳块、停止/重播、在途读取取消及过期失败抑制、速率/比例修改、终态错误。
真实 PreparedTransfer 的同 ID、SHA 和编码帧在循环及停止重播后逐字节相同。
UI 测试覆盖按钮、进度、准备取消、换文件时准备/读取乱序完成、背景提示，以及 pagehide 后继续使用。

新增端到端场景直接运行播放器并生成 PNG；随机文件从三轮播放中只捕获帧位置
`0,1,1,3,5,9,10,12`，模拟丢帧和重复，Rust 恢复 4/4 个块，输出逐字节相同、SHA 一致。
空文件也从自动播放的元数据正确完成。两种情况下接收成功后，发送器继续播放，直至测试显式 stop。
清单仍仅包含图像路径，接收器没有得到侧通道元数据或反馈。

调度测试使用可控时钟，UI 使用 DOM 适配模拟；它们不证明实际 Firefox 115 ESR 的绘制速率或 VNC 吞吐。
本轮未完成 Windows 7 / Firefox 115 ESR 及实际 VNC 连续接收验证。下一步刷新发送页面，从 5 FPS 开始，
在 GNOME 终端运行带 live-capture feature 的 receive，检查原始文件与输出 SHA 后再提高目标速率。


## 2026-10-17：用户桌面连续接收通过

用户在 GNOME 桌面终端执行：

```bash
cargo run --offline --locked --release \
  --manifest-path receiver/Cargo.toml --features live-capture -- \
  receive --output-dir target/phase2-received --source window
```

接收器退出码 0，状态 `Complete`，成功后自动停止采集并保存一次：

- 文件：`target/phase2-received/mcp-registry.json`。
- 40/40 个唯一数据块，158173 字节；50 张有效帧。
- 收到元数据前忽略 9 张数据帧，随后正常初始化并完成。
- CRC、定位、调色板、无效帧、外来帧、重复帧计数均为 0。
- PipeWire buffers 为 50，最新帧队列 overwritten 为 0。
- 接收指标耗时 10.105548234 秒，唯一块速率 3.9582216693 块/秒，有效文件速率 15652.094902464447 字节/秒；耗时包含处理及最终哈希。

另运行 `stat -c '%s bytes %n' target/phase2-received/mcp-registry.json` 和
`sha256sum target/phase2-received/mcp-registry.json` 独立核对输出：长度 158173 字节，SHA-256 为
`daa1fe6648724435b2c00766f50df5ec178028ab6de6f0f01ee1cb2606b292e3`，与此前发送页面显示的哈希一致。

本条记录确认当前桌面窗口的连续接收链路。此次日志没有说明源机、浏览器版本、是否经过 VNC 或请求 FPS，
因此不作为 Windows 7 / Firefox 115 ESR 经 VNC 的验收，也不是六档速率基准。上述实测结果补充此前仅有截图和离线测试的记录。


## 2026-10-17：15 FPS 桌面接收记录

用户指定发送速率 15 FPS，输出目录 `target/received-15fps`。接收退出码 0，40/40 块、158173 字节，
52 张有效帧，元数据前忽略 11 张数据帧，CRC/定位/调色板/无效/重复/外来帧计数均为 0，
PipeWire buffers=52、overwritten=0。指标耗时 3.805640237 秒（含处理和最终哈希），
有效文件速率 41562.78317171892 字节/秒，唯一块速率 10.510715020065097 块/秒。
独立 stat/sha256sum 确认长度和 SHA-256 与此前输出相同：
`daa1fe6648724435b2c00766f50df5ec178028ab6de6f0f01ee1cb2606b292e3`。
此前同目录重跑因同名输出存在而被拒绝，属于防覆盖行为，不作为 15 FPS 接收失败样本。
该记录仍未说明远端平台及 VNC 链路，不推断为完整 VNC 基准。


## 2026-10-17：三档数据矩阵实现验证

新增 profile 2（160×160）和 profile 3（256×256），profile 1 保持原二进制兼容。
独立 Python 固定向量扩展至 88 个文件，包含两档新增尺寸的空文件、整块边界、两个整块加 17 字节末块和 Unicode 文件名。

执行命令与结果：

```bash
python3 tests/generate-vectors.py --check
node tests/js/codec.test.cjs
node tests/js/player.test.cjs
node tests/js/ui.test.cjs
cargo test --offline --locked --manifest-path receiver/Cargo.toml --features live-capture
node tests/e2e.cjs
cargo clippy --offline --locked --manifest-path receiver/Cargo.toml --all-targets --features live-capture -- -D warnings
cargo fmt --manifest-path receiver/Cargo.toml -- --check
cargo build --offline --locked --release --manifest-path receiver/Cargo.toml --features live-capture
```

- 固定向量检查通过；JS codec 24、player 13、UI 7，共 44 项通过。
- Rust 8 项单元测试、21 项集成测试，共 29 项通过；Clippy、格式检查通过。
- PNG 端到端 42 个场景通过，产物 `target/e2e-4kgpyG/`；三个 profile 均逐字节恢复且 SHA 一致。
- 大矩阵覆盖组合缩放/旋转/颜色扰动、混合帧 CRC 拒绝、漏帧/重复、跨循环补块和真实自动播放器生成的 PNG。
- 新增校验拒绝 CRC 正确但 profile 与帧长/元数据块容量冲突的帧；即使同 ID、单个短块，也不能混合 profile。
- UI 覆盖选尺寸后准备文件、准备/播放/暂停时锁定、停止后换尺寸重新准备，以及相应空白画布尺寸。

组合失真测试采用与原测试相同的 4 像素/符号。初次使用 3 像素/符号的大矩阵在双线性非等比缩放
（1.13×0.875）、旋转及颜色扰动叠加时出现调色板拒绝（失败产物 `target/e2e-7vzeuf/`）；
保持原校准阈值，未把拒绝帧作为有效帧接收。两档新尺寸的真实 VNC 接收仍需实测。

## 2026-10-17：新增大矩阵后的两次桌面连续接收

用户运行带 `live-capture` 的 `receive --source window`，两次均退出码 0，自动完成并保存一次。
由文件大小和块数可推断两次均使用了 profile 3（每块最多 16316 字节）；日志没有单独记录发送页面的矩阵选项、实际 VNC 链路或浏览器版本。

| 输出 | 文件字节数 | 唯一块 | 图像帧 | 等待元数据 | 重复块 | 接收耗时 | 有效文件速率 |
|---|---:|---:|---:|---:|---:|---:|---:|
| `target/received-15fps2/mcp-registry.json` | 158173 | 10/10 | 19 | 8 | 0 | 1.366928022 秒 | 115714.2128 字节/秒 |
| `target/received-15fps3/OVMF_CODE_4M.fd` | 3653632 | 224/224 | 322 | 35 | 58 | 17.532683556 秒 | 208389.7761 字节/秒 |

两次 CRC、定位、调色板、无效帧和外来帧计数均为 0；PipeWire 队列 overwritten 均为 0。
第二次的 58 个重复块被正确去重。独立读取落盘文件核对：第一个 SHA-256 为
`daa1fe6648724435b2c00766f50df5ec178028ab6de6f0f01ee1cb2606b292e3`；第二个为
`624e06de18b4fa535e90db7160d00d3d07d206422b89999bf1e27d920264e4e0`。
接收器报告的 `Complete` 还包含最终长度和发送端 SHA-256 校验。
