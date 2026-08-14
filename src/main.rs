//! rust-musicfox — NetEase Cloud Music TUI client (Rust rewrite of
//! go-musicfox). Binary entry point; all logic lives in the `rust_musicfox`
//! library crate.
//!
//! CLI:
//!   musicfox                        启动 TUI
//!   musicfox --cookie "k=v; k2=v2"  直接以 cookie 登录后启动

use anyhow::Result;
use rust_musicfox::{api, player, ui};

#[tokio::main]
async fn main() -> Result<()> {
    // Restore the terminal even on panics.
    std::panic::set_hook(Box::new(|info| {
        ratatui::restore();
        eprintln!("panic: {}", info);
    }));

    let mut cookie_arg: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--cookie" | "-c" => {
                cookie_arg = args.next();
            }
            "--help" | "-h" => {
                println!(
                    "rust-musicfox — 网易云音乐 TUI 客户端\n\n\
                     用法: musicfox [--cookie \"k=v; k2=v2\"]\n\
                     \n\
                     登录方式:\n  \
                     1. 应用内扫码登录（部分网络不可用）\n  \
                     2. 应用内 Cookie 登录：将浏览器 MUSIC_U cookie 写入\n     \
                     {} 后按 r\n  \
                     3. 命令行: musicfox --cookie \"MUSIC_U=xxxx\"",
                    api::data_dir().join("cookie.txt").display()
                );
                return Ok(());
            }
            _ => {
                eprintln!("未知参数: {} （--help 查看用法）", arg);
                std::process::exit(2);
            }
        }
    }

    let cookie_path = api::data_dir().join("cookies.json");
    let mut client = api::NeteaseClient::new(Some(cookie_path))?;
    if let Some(cookie) = cookie_arg {
        match client.set_cookie_str(&cookie) {
            Ok(()) => println!("已导入 cookie，登录状态: 已登录"),
            Err(e) => {
                eprintln!("cookie 导入失败: {}", e);
                std::process::exit(1);
            }
        }
    }
    let player = player::Player::new()?;
    let app = ui::App::new(client, player)?;
    app.run().await
}
