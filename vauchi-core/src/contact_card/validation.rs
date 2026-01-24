//! Field Validation
//!
//! TODO: Add validation functions for phone, email, and other field types.
//! Currently only defines error types - validators will be added as needed.

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
