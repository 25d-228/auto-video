fn main() {
    // Compile the libtorrent-rasterbar C++ shim behind the cxx bridge (src/lt.rs).
    // Homebrew install prefix (Apple Silicon). Adjust for other layouts.
    let brew = "/opt/homebrew";
    cxx_build::bridge("src/lt.rs")
        .file("src/lt_shim.cpp")
        .flag_if_supported("-std=c++17")
        .include("src")
        .include(format!("{brew}/include"))
        .compile("av_lt_shim");

    // Link Homebrew's libtorrent (dynamic; its transitive boost/openssl deps are
    // resolved via the dylib's recorded install names at runtime).
    println!("cargo:rustc-link-search=native={brew}/lib");
    println!("cargo:rustc-link-lib=dylib=torrent-rasterbar");

    println!("cargo:rerun-if-changed=src/lt.rs");
    println!("cargo:rerun-if-changed=src/lt_shim.cpp");
    println!("cargo:rerun-if-changed=src/lt_shim.h");

    tauri_build::build();
}
