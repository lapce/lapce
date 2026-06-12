//! Benchmark evaluation framework.
//!
//! SWE-bench evaluation, performance profiling, and competitor comparison.

pub mod swe_runner;
pub mod perf_suite;

pub use swe_runner::{
    SweRunner, SweInstance, SweResult, BenchmarkConfig, BenchmarkReport,
    CompetitorScore, DifficultyStats, generate_sample_dataset,
};
pub use perf_suite::{
    PerfSuite, PerfReport, BenchmarkMeasurement, MachineInfo, PerfSummary,
    ComparisonResult, RegressionItem,
};
