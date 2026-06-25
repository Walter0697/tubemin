pub mod submit;
pub mod dashboard;
pub mod settings;

pub use submit::submit;
pub use dashboard::dashboard;
pub use settings::{settings, generate_key, revoke_key};
