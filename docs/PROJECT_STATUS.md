# rust-musicfox 项目状态与交接文档

> 最后更新：2026-08-14
> 本文档面向后续接手者（Linux 环境），汇总项目全貌、架构、关键决策、
> 已知限制与下一步规划。

## 1. 项目概述

**rust-musicfox** 是 [go-musicfox](https://github.com/go-musicfox/go-musicfox)
（Go + bubbletea 的网易云音乐 TUI 客户端，2500+ stars）的 Rust 重写版本。

- **定位**：网易云音乐终端客户端（TUI），纯 Rust，无 CGo/Node 依赖
- **技术栈**：Rust 2021 + `ratatui`(TUI) + `crossterm`(终端) + `tokio`(异步) +
  `reqwest`(HTTP) + `rodio`/`symphonia`(音频 mp3/flac/ogg/wav)
- **平台**：Linux / macOS / Windows 三平台（CI 全平台构建）
- **仓库**：https://github.com/PAKIKNOWLEDGE/rust-musicfox （fork 自 go-musicfox，
  默认分支 `master`）

## 2. 功能清单

### 已完成

| 功能 | 说明 |
|------|------|
| 扫码登录 | 明文 `/api/login/qrcode/*` 接口，绝大多数网络可用 |
| Cookie 登录 | 任意网络可用（浏览器 MUSIC_U 写入 cookie.txt 或 `--cookie` 参数） |
| 每日推荐 | 需登录 |
| 推荐歌单 | 首页推荐 |
| 我的歌单 | 需登录，`/api/nuser/account/get` + `/api/user/playlist` |
| 歌单广场 | 精选歌单，10 个分类切换（`c`） |
| 榜单 | 63 个官方榜单 |
| 搜索 | 歌曲搜索（`/` 输入） |
| 歌单全量加载 | 支持 2000+ 首大歌单（trackIds 分批并发） |
| 播放 | 下载即播；6 种播放模式（见下） |
| 播放队列 | `v` 查看，`x` 移除，Enter 跳转 |
| 播放状态持久化 | 重启恢复队列/当前歌曲/播放模式（playlist.json） |
| 歌词 | LRC 同步 + 翻译（tlyric），含切歌竞态防护 |
| 下载歌曲 | `d` 下载当前歌曲到 downloads/ |
| 音质切换 | `b` 128k/320k，持久化 |
| 配置持久化 | config.toml（音量/播放模式/码率） |
| 帮助页 | `?` 完整快捷键 |
| 无音频降级 | 无声卡时应用仍可浏览/搜索/下载 |
| 发布流程 | 打 `v*` tag → 三平台二进制 + GitHub Release |
| CI | fmt/clippy/test/build × 3 平台 |

### 播放模式（6 种，对齐 go-musicfox）

列表循环 / 顺序播放 / 单曲循环 / 列表随机（洗牌）/ 无限随机（历史避重）/
心动模式（暂为顺序播放占位，推荐接口依赖 weapi+登录，见限制）。

### 未完成 / 规划

- [ ] 心动模式真实推荐（依赖 weapi 接口，风控网络不可用）
- [ ] 歌词卡拉 OK（YRC）渲染
- [ ] 私人 FM
- [ ] 桌面歌词、MPRIS 远程控制、Last.fm
- [ ] 主题系统
- [ ] 封面图（Kitty/ueberzug 协议）

## 3. 架构

```
src/
├── main.rs      # 入口：--cookie / --version / --help，tokio runtime
├── lib.rs       # 库 crate（bin 与 examples 共用）
├── api/
│   ├── mod.rs   # API 客户端：请求封装、cookie 管理、登录
│   ├── types.rs # 响应结构（serde）
│   └── weapi.rs # weapi 加密（AES-CBC + 无填充 RSA）—— 仅保留参考，运行时不使用
├── config.rs    # 配置持久化（TOML）
├── lyric.rs     # LRC 解析
├── playlist.rs  # 播放列表管理器 + 6 种播放模式 + 状态快照
├── player.rs    # rodio 播放引擎（无音频设备降级）
└── ui.rs        # TUI：视图状态机 + 事件循环 + 渲染（~2000 行）
```

### 线程模型

- 单主 async 任务：持有 Player（音频设备只被主任务触碰）并负责渲染（30fps）
- 键盘事件：独立线程 `crossterm::event::read()` → channel
- 网络任务：`tokio::spawn`（歌单/搜索/QR 轮询/下载/歌词）→ `mpsc` channel 回传
  `Msg` 枚举 → 主任务 `handle_msg`
- 客户端 `NeteaseClient` 包在 `Arc<Mutex<>>` 中，网络任务持锁调用

### 关键消息流

```
Msg 枚举（ui.rs）：
  PlaylistReady / PlaylistListReady / MyPlaylistsReady / ToplistReady /
  SquareReady / SearchReady / PlaybackReady{song,bytes} /
  LyricsReady{song_id,lines,trans} / DownloadDone / OpError /
  QrKeyReady / QrStatus / LoginVerified / CookieLoginOk / LoggedOut
```

## 4. 关键技术决策（重要！）

### 4.1 全部走明文 `/api/` 接口，弃用 weapi

**背景**：网易云风控会拦截 `/weapi/` 路径（返回 200 空响应或 302），
在部分网络（含本项目开发机）上**真实浏览器也过不了**，与 TLS 指纹无关。
go-musicfox 用 uTLS 模拟 Chrome 指纹走 weapi；Rust 侧等价物
（reqwest-impersonate 等）已全部 yanked/停更，不可用。

**决策**：所有接口改用明文 legacy API（`/api/...`），实测：
- 搜索 `GET /api/search/get/web?s=&type=1&offset=0&limit=`
- 歌单详情 `GET /api/v3/playlist/detail?id=&n=100000&s=8`（**必须带 n/s**，
  否则 tracks 只返回 10 首）
- 歌曲详情 `GET /api/song/detail?ids=[...]`（**单次上限 ~201 首**，
  分块 200 并发）
- 歌词 `GET /api/song/lyric?id=&lv=-1&kv=-1&tv=-1`
- 歌曲 URL `GET /api/song/enhance/player/url?ids=[id]&br=`
- 榜单 `GET /api/toplist`；精选歌单 `GET /api/playlist/highquality/list?cat=`
- 用户歌单 `GET /api/user/playlist?uid=&limit=`；
  账号 `GET /api/nuser/account/get`
- QR 登录 `GET /api/login/qrcode/unikey?type=1&noCheckToken=true` +
  `GET /api/login/qrcode/client/login?type=1&noCheckToken=true&key=`
  （轮询返回 801 等待 / 802 已扫码 / 803 成功，**不要用 code==200 校验**）

`weapi.rs` 保留（含测试）作为参考，运行时无调用。

### 4.2 大歌单全量加载（对齐 go-musicfox PlaylistTrackAllService）

`detail` 的 `tracks` 截断但 `trackIds` 全量 →
每 200 个 id 并发查 `song/detail` → 按 trackIds 顺序重排。

### 4.3 播放系统（对齐 go-musicfox internal/playlist）

`PlaylistManager`：current + playlist + mode；6 种模式实现 next/prev
（带 `manual` 标志区分手动/自动）；`remove_song` 调整当前索引；
`save_state/load_state` 存 `playlist.json`。

### 4.4 登录

- 扫码：明文 QR 接口（主推）
- Cookie：`cookie.txt`（应用内 `c` → `r`）或 `musicfox --cookie "MUSIC_U=xxx"`
- 803 后调 `account/get` 验证登录（LoginVerified），失败提示改用 Cookie

## 5. 已知限制

1. **weapi 系功能不可用**：心动模式推荐、部分需 weapi 的接口（风控网络下）
2. **搜索仅英文/拼音输入**：crossterm 不处理 IME 中文输入
3. **扫码登录依赖网络**：明文 `/api/` 路径若被拦（极少数网络），用 Cookie 登录
4. **音质**：320k 对 VIP 歌曲返回无 URL（提示"no playable url"）
5. **智能模式**：心动模式目前行为等同顺序播放
6. **下载是整首入内存**：大文件（320k 无损）占内存，未做流式落盘

## 6. 构建与运行

### Linux 依赖

```bash
# Debian/Ubuntu
sudo apt install -y libasound2-dev pkg-config build-essential
# 音频运行时（多数发行版自带 ALSA）
```

### 构建

```bash
cargo build --release
./target/release/musicfox
# 或 cargo run
```

### Nix

```bash
# 可复现源码构建
nix build .# && nix profile install ./result
# 二进制打包（binary-base 模式，来自 skill-nix-binary-base-packing-creator）
cargo build --release && nix-build ./packaging/nix-binary.nix && nix profile install ./result
```

flake.nix 已含 Linux ALSA 依赖（nativeBuildInputs: pkg-config; buildInputs: alsa-lib）。

### 数据目录

- Linux: `~/.config/rust-musicfox/`（`cookies.json` / `cookie.txt` / `config.toml` /
  `playlist.json` / `downloads/`）

### 快捷键速查

```
全局: ↑↓/jk 移动  Enter 进入  Esc 返回  Ctrl+C 退出  ? 帮助
播放页: 空格 暂停/播放  s 停止  n/p 下一首/上一首  ↑↓ 快进快退5s
        +/- 音量  m 播放模式  b 音质  d 下载  v 队列  q 返回
队列: Enter 播放选中  x 移除
搜索: / 输入  Enter 搜索
登录: q 扫码模式  c Cookie模式  r 刷新/读取
歌单广场: c 切换分类
```

## 7. 发布流程

```bash
git tag vX.Y.Z && git push origin vX.Y.Z
# GitHub Actions Release 工作流：三平台构建 + 自动创建 Release
# 首次/权限问题：workflow 已设 permissions: contents: write
```

## 8. 测试与质量

- 19 个单元测试（weapi 加密、LRC 解析、播放模式、歌单类型反序列化、配置往返）
- CI：fmt / clippy(-D warnings) / test / build × ubuntu/windows/macos
- 冒烟工具（examples/）：
  - `probe`：全接口实时探针（搜索/榜单/歌单/QR/账号）
  - `fullplaylist`：大歌单全量加载验证
  - `playtest`：音频播放端到端验证

## 9. 交接备注

- 开发机为 Windows，网络偶发 GitHub 不通（clash 7890 端口可用时走代理）
- 网易云接口会风控：改接口前先用 curl 实测（带 Referer + UA，注意 curl 的
  `[]` 需 `-g`）
- 大改动前先读 go-musicfox 对应实现（`reference/go-musicfox/` 为本地参考副本）
- 保持"明文接口优先"原则，weapi 系接口默认视为不可用
