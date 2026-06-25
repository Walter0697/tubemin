mod config;
mod db;
mod api_keys;
mod metube;
mod state;
mod handlers;
mod watcher;

#[tokio::main]
async fn main() {
    let _config = config::Config::from_env().expect("Failed to load config");
    println!("Tubemin starting");
}
