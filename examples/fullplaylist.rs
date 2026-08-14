use rust_musicfox::api::NeteaseClient;

fn main() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let client = NeteaseClient::new(None).unwrap();
        // 686-song playlist found via search
        match client.playlist_all_tracks(2729962695).await {
            Ok(songs) => println!("playlist_all_tracks OK: {} songs", songs.len()),
            Err(e) => println!("playlist_all_tracks FAILED: {e}"),
        }
        // 200-song top list (regression: should now also be full)
        match client.playlist_all_tracks(3778678).await {
            Ok(songs) => println!("toplist all tracks OK: {} songs", songs.len()),
            Err(e) => println!("toplist all tracks FAILED: {e}"),
        }
    });
}
