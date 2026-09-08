//! Fine-tuning Framework - Project-specific model adaptation.
//!
//! This module provides tools for fine-tuning language models on project-specific code.

pub mod lora_tuner;
pub mod lora_engine;

pub use lora_engine::{PythonBridgeConfig, train_with_python};
pub use lora_tuner::{
    CodeSample,
    FineTuneDataset,
    LoRAConfig,
    FineTuneResult,
    DatasetStats,
    LoRATuner,
    EvaluationResult,
};
