//! Live API probe: hits the real NetEase server through the same code paths
//! the TUI uses. Not run in CI.
//!
//! Usage: cargo run --example probe

use rust_musicfox::api::weapi;

fn main() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let client = rust_musicfox::api::NeteaseClient::new(None).unwrap();

        // 1) weapi params shape
        let (params, enc) = weapi::weapi_params(&serde_json::json!({"csrf_token": ""}));
        println!(
            "params len: {} (b64), encSecKey len: {} (hex)",
            params.len(),
            enc.len()
        );

        // 2) search (legacy plain API)
        match client.search("周杰伦", 3).await {
            Ok(songs) => {
                println!("search OK, {} songs", songs.len());
                for s in songs.iter().take(3) {
                    println!(
                        "  #{} {} - {} [{}]",
                        s.id,
                        s.name,
                        s.artist_names(),
                        s.album_name()
                    );
                }
                if let Some(s) = songs.first() {
                    // 3) song url
                    match client.song_url(s.id, 128000).await {
                        Ok(u) => println!(
                            "song_url OK (len {}): ...{}",
                            u.len(),
                            &u[u.len().saturating_sub(40)..]
                        ),
                        Err(e) => println!("song_url FAILED: {e}"),
                    }
                    // 4) lyric
                    match client.lyric(s.id).await {
                        Ok(l) => println!(
                            "lyric OK: {:?} chars",
                            l.lrc.map(|b| b.lyric.len()).unwrap_or(0)
                        ),
                        Err(e) => println!("lyric FAILED: {e}"),
                    }
                }
            }
            Err(e) => println!("search FAILED: {e}"),
        }

        // 5) personalized playlists
        match client.personalized_playlists(3).await {
            Ok(items) => println!(
                "personalized OK, {} items: {:?}",
                items.len(),
                items.iter().map(|i| i.name.clone()).collect::<Vec<_>>()
            ),
            Err(e) => println!("personalized FAILED: {e}"),
        }

        // 5b) top lists
        match client.toplists().await {
            Ok(items) => println!(
                "toplists OK, {} items: {:?}",
                items.len(),
                items
                    .iter()
                    .take(3)
                    .map(|t| t.name.clone())
                    .collect::<Vec<_>>()
            ),
            Err(e) => println!("toplists FAILED: {e}"),
        }

        // 5c) playlist square (high quality)
        match client.highquality_playlists("华语", 3).await {
            Ok(items) => println!(
                "square OK, {} playlists: {:?}",
                items.len(),
                items
                    .iter()
                    .take(3)
                    .map(|p| p.name.clone())
                    .collect::<Vec<_>>()
            ),
            Err(e) => println!("square FAILED: {e}"),
        }

        // 6) playlist detail (云音乐热歌榜 id=3778678)
        match client.playlist_detail(3778678).await {
            Ok(p) => println!(
                "playlist OK: {} tracks={:?} playCount={:?}",
                p.name, p.track_count, p.play_count
            ),
            Err(e) => println!("playlist FAILED: {e}"),
        }

        // 7) QR key (new endpoint + full browser headers, mirroring go-musicfox)
        match client.qr_key().await {
            Ok(key) => {
                println!("qr_key OK: {key}");
                println!("qr_url: {}", client.qr_url(&key));
            }
            Err(e) => println!("qr_key FAILED: {e}"),
        }
    });
}
