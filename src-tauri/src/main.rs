// Always a GUI app (no console window popping up, even in debug builds).
#![windows_subsystem = "windows"]

fn main() {
    auto_video_lib::run();
}
