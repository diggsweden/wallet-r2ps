use dotenvy::dotenv;

pub mod kafka;

pub fn init() {
    dotenv().ok();
}
