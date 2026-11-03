//! waybot — otomatisasi kuis Wayground lewat Firefox nyata.
#![allow(dead_code)]

mod anticheat;
mod app;
mod browser;
mod config;
mod events;
mod model;
mod solver;
mod store;
mod tui;
mod util;
mod wayground;

use config::{AppConfig, parse_args};

fn main() {
    dotenvy::dotenv().ok();
    let cli = parse_args();
    let app = match AppConfig::load(&cli) {
        Ok(app) => app,
        Err(message) => {
            eprintln!("konfigurasi: {message}");
            std::process::exit(2);
        }
    };

    if cli.print_config {
        println!("base_url={}", app.base_url);
        println!("model={}", app.model);
        println!("thinking={:?}", app.thinking);
        println!(
            "vision={} model={}",
            app.vision,
            if app.vision_model.is_empty() {
                "-"
            } else {
                &app.vision_model
            }
        );
        println!("jeda={}..{}ms", app.jeda_min_ms, app.jeda_maks_ms);
        println!("key={}", if app.has_key() { "terbaca" } else { "absen" });
        return;
    }

    if let Err(e) = tui::run(cli, app) {
        eprintln!("tui: {e}");
        std::process::exit(1);
    }
}
