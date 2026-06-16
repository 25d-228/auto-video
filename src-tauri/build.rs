fn main() {
    // Torrent backend is now pure-Rust (librqbit) — no C++ shim to compile and
    // no external libtorrent dylib to link.
    tauri_build::build();
}
