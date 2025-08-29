mod states;
mod api;
mod rooms;

pub use rooms::*;

use std::sync::Mutex;
use crate::scaffolding;
pub use states::ExceptionType;
pub use api::*;

pub static SCAFFOLDING_PORT: Mutex<u16> = Mutex::new(0);

pub fn initialize() {
    let port = scaffolding::server::start(experimental::HANDLERS, 13448)
        .unwrap_or_else(|_| scaffolding::server::start(experimental::HANDLERS, 0).unwrap());

    let mut v = SCAFFOLDING_PORT.lock().unwrap();
    *v = port;
}
