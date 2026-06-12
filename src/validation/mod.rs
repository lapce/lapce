//! Validation module — API validation and correctness checking.
pub mod api_validation;

pub use api_validation::{
    ApiValidator, ValidationConfig, ValidationResult,
    CostComparison,
};
