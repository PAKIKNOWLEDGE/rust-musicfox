//! Playback smoke test: search a song, download it, and play 3 seconds
//! through the real audio pipeline. Requires an audio device.
//!
//! Usage: cargo run --example playtest

fn main() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let client = rust_musicfox::api::NeteaseClient::new(None).unwrap();
        let songs = match client.search("晴天", 1).await {
            Ok(s) => s,
            Err(e) => {
                println!("search failed: {e}");
                return;
            }
        };
        let song = &songs[0];
        println!("playing: {} - {}", song.name, song.artist_names());
        let url = match client.song_url(song.id, 128000).await {
            Ok(u) => u,
            Err(e) => {
                println!("song url failed: {e}");
                return;
            }
        };
        let bytes = match client.http().get(&url).send().await {
            Ok(r) => match r.bytes().await {
                Ok(b) => b.to_vec(),
                Err(e) => {
                    println!("download failed: {e}");
                    return;
                }
            },
            Err(e) => {
                println!("download failed: {e}");
                return;
            }
        };
        println!("downloaded {} KB", bytes.len() / 1024);
        let mut player = match rust_musicfox::player::Player::new() {
            Ok(p) => p,
            Err(e) => {
                println!("no audio device: {e}");
                return;
            }
        };
        match player.play_bytes(bytes) {
            Ok(()) => println!("playback started OK"),
            Err(e) => {
                println!("playback failed: {e}");
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(3));
        println!(
            "after 3s: state={:?} pos={}s",
            player.state(),
            player.position().as_secs()
        );
        player.stop();
        println!("playtest OK");
    });
}
