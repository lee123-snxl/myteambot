pub mod bots;
pub mod chat;
pub mod folder;
pub mod font;
pub mod group;
pub mod http_client;
pub mod image_gen;
pub mod media;
pub mod providers;
pub mod speech;
pub mod usage;
pub mod video;
#[cfg(windows)]
pub mod pick_folder;
pub mod proxy;
pub mod store;

#[cfg(feature = "gui")]
mod ui;

#[cfg(feature = "gui")]
pub fn run() -> eframe::Result<()> {
    ui::launch()
}
