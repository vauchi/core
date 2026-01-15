//! Field Validation
//!
//! TDD: Stub implementation - tests will drive full implementation.

use thiserror::Error;

/// Validation error types.
#[derive(Error, Debug)]
pub enum ValidationError {
    #[error("Invalid phone number format")]
    InvalidPhone,
    #[error("Invalid email format")]
    InvalidEmail,
    #[error("Value too long (max {max} characters)")]
    ValueTooLong { max: usize },
    #[error("Value cannot be empty")]
    EmptyValue,
}
