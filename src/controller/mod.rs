mod states;
mod api;
mod rooms;

pub use rooms::*;

use crate::scaffolding;
pub use states::ExceptionType;
pub use api::*;

lazy_static::lazy_static! {
    pub static ref SCAFFOLDING_PORT: u16 = scaffolding::server::start(experimental::HANDLERS, 13448)
        .unwrap_or_else(|_| scaffolding::server::start(experimental::HANDLERS, 0).unwrap());
}
