# rust-musicfox

[![CI](https://github.com/PAKIKNOWLEDGE/rust-musicfox/actions/workflows/ci.yml/badge.svg)](https://github.com/PAKIKNOWLEDGE/rust-musicfox/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/PAKIKNOWLEDGE/rust-musicfox?label=release)](https://github.com/PAKIKNOWLEDGE/rust-musicfox/releases)
[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

网易云音乐终端客户端（TUI），使用 Rust 编写。这是 [go-musicfox](https://github.com/go-musicfox/go-musicfox) 的 Rust 重写版本。

- 纯 Rust 实现，无 CGo / Node 依赖
- 跨平台：Linux / macOS / Windows
- TUI 基于 [ratatui](https://github.com/ratatui/ratatui)，音频解码基于 [rodio](https://github.com/RustAudio/rodio)（symphonia：mp3 / flac / ogg / wav）

## 功能

- [x] 扫码登录 + Cookie 粘贴登录（部分网络 weapi 被风控时使用 Cookie 模式）
- [x] 每日推荐（需登录）
- [x] 推荐歌单
- [x] 歌单广场（精选歌单，`c` 切换分类）
- [x] 榜单（飙升榜/新歌榜/原创榜等 63 个）
- [x] 搜索（歌曲）
- [x] 歌单详情 / 播放队列（自动连播，`v` 查看队列）
- [x] 播放模式：列表循环 / 单曲循环 / 随机 / 顺序（`m` 切换）
- [x] 音质切换：128k / 320k（`b`，持久化）
- [x] 无音频设备时降级运行（可浏览/搜索/下载，播放时报错提示）
- [x] 播放控制：播放/暂停/停止/上一首/下一首/快进快退/音量
- [x] 下载当前歌曲（`d`，存到 `<config>/rust-musicfox/downloads/`）
- [x] LRC 歌词同步显示 + 翻译歌词
- [x] 退出登录
- [x] 配置持久化（音量、播放模式、码率，存于 `config.toml`）
- [x] 帮助页（`?`）
- [x] 发布流程：打 `v*` tag 自动构建三平台二进制并创建 GitHub Release
- [ ] 歌词卡拉 OK 模式（YRC）
- [ ] 私人 FM
- [ ] 桌面歌词、MPRIS / 远程控制、Last.fm
- [ ] 主题自定义

## 安装

### 直接下载（推荐）

从 [Releases](https://github.com/PAKIKNOWLEDGE/rust-musicfox/releases) 下载对应平台的二进制，解压后直接运行：

```bash
# Linux / macOS
chmod +x rust-musicfox-Linux   # 或 rust-musicfox-macOS
./rust-musicfox-Linux

# Windows
# 运行 rust-musicfox-Windows.exe（在终端中）
```

### 源码构建

需要 Rust stable（推荐最新版）：

```bash
cargo build --release
./target/release/musicfox
```

或直接运行：

```bash
cargo run
```

### Nix

可复现的源码构建（flake）：

```bash
nix build .#
nix profile install ./result
```

二进制打包模式（不编译，直接打包本地构建产物，来自
[skill-nix-binary-base-packing-creator](https://github.com/PAKIKNOWLEDGE/skill-nix-binary-base-packing-creator)
的 binary-base 模式）：

```bash
cargo build --release
nix-build ./packaging/nix-binary.nix
nix profile install ./result
```

## 登录

两种方式，应用内登录页按 `q`/`c` 切换：

- **扫码登录（`q`，推荐）**：走明文 `/api/login/qrcode/*` 接口，不依赖 weapi，
  绝大多数网络可用。用手机网易云音乐 App 扫码即可。
- **Cookie 登录（`c`）**：任何网络都可用，适合扫码异常时备用。
  1. 浏览器打开 https://music.163.com 并登录
  2. F12 → 应用程序(Application) → Cookie → `https://music.163.com`
  3. 复制 `MUSIC_U` 的值，写入 `<config>/rust-musicfox/cookie.txt`
  4. 应用内按 `r` 读取并登录
  - 或命令行：`musicfox --cookie "MUSIC_U=xxx"`

## 使用

| 按键 | 功能 |
|------|------|
| `↑` / `↓` 或 `k` / `j` | 移动光标 |
| `Enter` | 进入 / 播放 |
| `Esc` | 返回上一页 / 退出输入模式 |
| `空格` | 播放 / 暂停 |
| `s` | 停止 |
| `n` / `→` | 下一首 |
| `p` / `←` | 上一首 |
| `↑` / `↓`（播放页） | 快进 / 快退 5 秒 |
| `+` / `-` | 音量加减 |
| `m` | 切换播放模式（列表循环/单曲/随机/顺序） |
| `b` | 切换音质（128k/320k） |
| `v` | 播放队列 |
| `?` | 帮助 |
| `/`（搜索页） | 进入搜索输入 |
| `r`（登录页） | 刷新二维码 / 读取 cookie |
| `Ctrl+C` | 退出 |

完整快捷键见应用内帮助页（`?`）。

## 配置与数据

- 配置文件目录：`<config>/rust-musicfox/`（Linux: `~/.config/rust-musicfox/`，
  macOS: `~/Library/Application Support/rust-musicfox/`，
  Windows: `%APPDATA%\rust-musicfox\`）
- `cookies.json` — 登录 cookie，登录状态重启后保留
- `cookie.txt` — Cookie 登录时手动写入的 MUSIC_U cookie（应用内按 `r` 读取）
- `config.toml` — 音量、播放模式、码率等设置

## 架构

```
src/
├── main.rs      # 入口（--cookie 参数、--help），tokio runtime
├── lib.rs       # 库 crate（bin 与 examples 共用）
├── api/         # 网易云 API 客户端
│   ├── mod.rs   # 请求封装（QR/ Cookie 登录、搜索、歌单、歌曲 URL、歌词）
│   ├── types.rs # 响应结构
│   └── weapi.rs # weapi 加密（AES-128-CBC + 无填充 RSA，对齐 Go 参考实现）
├── config.rs    # 配置持久化（TOML）
├── player.rs    # rodio 播放引擎
├── lyric.rs     # LRC 歌词解析
└── ui.rs        # TUI：状态机 + 事件循环 + 渲染
```

UI 线程模型：单主任务持有播放器并负责渲染；按键事件由独立线程读取经 channel
送入；网络请求（歌单/搜索/QR/下载）在 tokio 任务中执行，结果经 channel 回传主
任务。

## 与 go-musicfox 的差异

- 播放引擎：rodio（纯 Rust）替代 beep + 平台引擎；不支持 DLNA / MPD / MPV / 桌面歌词
- 登录：支持扫码（weapi）与 Cookie 粘贴两种方式，未实现网页 WebView 登录
- 存储：JSON cookie + TOML 配置替代 BoltDB
- 未实现：远程控制（MPRIS 等）、频谱、主题系统、Last.fm 同步

## 相关项目

- [go-musicfox](https://github.com/go-musicfox/go-musicfox) — 本项目重写的原版（Go + bubbletea）
- [akirco/pigma](https://github.com/akirco/pigma) — Ratatui 构建的网易云音乐 TUI 客户端
- [professor-lee/CNMPlayer](https://github.com/professor-lee/CNMPlayer) — 带音频可视化的网易云 TUI 播放器

## License

[MIT](LICENSE)
