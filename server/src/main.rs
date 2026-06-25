mod config;
mod db;
mod api_keys;

#[tokio::main]
async fn main() {
    let _config = config::Config::from_env().expect("Failed to load config");
    println!("Tubemin starting");
}
