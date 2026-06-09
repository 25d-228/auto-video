// Smoke test: prove librqbit can take a magnet, connect to peers, and resolve the
// torrent metadata (= the download path works). Stops as soon as metadata arrives —
// it does NOT download the content. Run: cargo run --example dl_test -- "<magnet>"
#[tokio::main]
async fn main() {
    let magnet = std::env::args().nth(1).expect("pass a magnet as the first argument");
    let dir = std::env::temp_dir().join("av_dl_test");
    let _ = std::fs::create_dir_all(&dir);
    println!("output dir: {}", dir.display());

    let session = librqbit::Session::new(dir)
        .await
        .expect("session::new failed");
    let resp = session
        .add_torrent(
            librqbit::AddTorrent::from_url(&magnet),
            Some(librqbit::AddTorrentOptions {
                overwrite: true,
                ..Default::default()
            }),
        )
        .await
        .expect("add_torrent failed");
    let handle = resp.into_handle().expect("no torrent handle");

    for t in 0..40 {
        let st = handle.stats();
        let speed = st
            .live
            .as_ref()
            .map(|l| l.download_speed.mbps)
            .unwrap_or(0.0);
        println!(
            "t={:2}s  total={} bytes  progress={} bytes  peers_live={}  speed={:.2} Mbps  state={:?}",
            t, st.total_bytes, st.progress_bytes,
            st.live.as_ref().map(|l| l.snapshot.peer_stats.live).unwrap_or(0),
            speed, st.state
        );
        if st.total_bytes > 0 {
            println!(
                "\nMETADATA RESOLVED ✓  librqbit connected and read the torrent ({} bytes total). Download engine works.",
                st.total_bytes
            );
            return;
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    println!("\nno metadata within 40s (could be a slow/dead magnet) — engine ran without error though");
}
