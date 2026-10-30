#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    install_panic_log();
    if let Err(err) = local_bot::run() {
        let dir = local_bot::store::default_data_dir();
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("boot-error.txt"), err.to_string());
    }
}

fn install_panic_log() {
    let dir = local_bot::store::default_data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("panic.log");
    std::panic::set_hook(Box::new(move |info| {
        let _ = std::fs::write(&path, format!("{info}"));
    }));
}
