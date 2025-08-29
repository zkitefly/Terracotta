mod room;
mod protocols;

use rand_core::{OsRng, TryRngCore};
pub use room::*;
pub use protocols::*;

lazy_static::lazy_static! {
    static ref MACHINE_ID: &'static str = {
        let mut bytes = [0u8; 16];
        OsRng.try_fill_bytes(&mut bytes).unwrap();
        Box::leak(Box::new(hex::encode(bytes)))
    };
}