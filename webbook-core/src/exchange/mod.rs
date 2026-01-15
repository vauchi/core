//! Contact Exchange Module
//!
//! Handles peer-to-peer contact exchange via QR codes, audio proximity,
//! and X3DH key agreement.

mod error;
mod qr;
mod x3dh;

pub use error::ExchangeError;
pub use qr::ExchangeQR;
pub use x3dh::{X3DH, X3DHKeyPair};
