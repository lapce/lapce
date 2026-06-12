//! Testing framework — unit, integration, and regression test infrastructure.

pub mod integration;

pub use integration::{
    TestWorkspace, IntegrationTestResult, AssertionResult,
    IntegrationHarness, IntegrationTestCase, TestCategory, TestSummary,
    agent_loop_test, rag_retrieval_test, batch_editor_atomicity_test,
    cache_stability_test, security_sanitizer_test, cost_budget_enforcement_test,
    // Phase 3: velobase/velobase-harness enhancements
    TestParams, ParametrizedTestCase, ParametrizedTestResult,
    HarnessConfig, TestFixture, FixtureRegistry,
};
