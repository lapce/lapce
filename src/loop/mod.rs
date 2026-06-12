//! Unified Agent Loop — Observe → Clarify → Plan → Act → Evaluate.
//!
//! A generic 5-phase loop engine that powers both code review and browser testing.
//!
//! ## Architecture
//!
//! ```text
//! LoopEngine<S, P, A, E>
//!   │
//!   ├── observe()  ──→ S: Observer     (sensor: code scanner / Playwright)
//!   ├── clarify()  ──→ (optional) Clarifier (question-answer)
//!   ├── plan()     ──→ P: Planner      (LLM decision maker, with constitution context)
//!   ├── act()      ──→ A: Actor        (actuator: file editor / browser)
//!   ├── evaluate() ──→ E: Evaluator    (judge: compiler / visual diff)
//!   │
//!   └── loop() runs up to `max_rounds` with feedback
//!         Observe → Clarify → Plan → Act → Evaluate → [retry?] → Observe → ...
//! ```
//!
//! ## Modes
//!
//! - [`ReviewMode`](crate::review::ReviewMode): code review (Observe=scan, Act=edit, Evaluate=compile)
//! - [`TestMode`](crate::test::TestMode): browser E2E testing (Observe=playwright, Act=click, Evaluate=visual diff)

use std::time::Instant;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::rules::constitution::Constitution;
use crate::rules::spec::SpecDelta;

// ============================================================================
// Phase enumeration
// ============================================================================

/// The phases of a single loop iteration (with optional Clarify step).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoopPhase {
    /// Sense the environment (scan code / capture screenshot).
    Observe,
    /// Optionally: resolve ambiguities before planning.
    Clarify,
    /// Decide next action (LLM inference / heuristic rule).
    Plan,
    /// Execute the planned action (apply edit / click button).
    Act,
    /// Judge whether the action succeeded (compile check / visual diff).
    Evaluate,
}

impl LoopPhase {
    pub fn next(&self) -> Option<LoopPhase> {
        match self {
            LoopPhase::Observe => Some(LoopPhase::Clarify),
            LoopPhase::Clarify => Some(LoopPhase::Plan),
            LoopPhase::Plan => Some(LoopPhase::Act),
            LoopPhase::Act => Some(LoopPhase::Evaluate),
            LoopPhase::Evaluate => None, // end of round
        }
    }
}

// ============================================================================
// LoopRole — role-specific modes (inspired by gstack's role separation)
//
// Maps gstack's 9 specialist roles onto LoopEngine's cognitive modes.
// Each role produces a distinct system-prompt suffix that biases the
// LLM toward a specific engineering perspective.
//
// | gstack Skill       | LoopRole Variant      | Phase Focus          |
// |---------------------|-----------------------|----------------------|
// | /plan-ceo-review    | Founder               | Clarify (product)    |
// | /plan-eng-review    | Architect             | Plan (architecture)  |
// | /review             | Reviewer              | Evaluate (paranoid)  |
// | /ship               | ReleaseEngine         | Act (shipping)       |
// | /browse, /qa        | QualityAssurance      | Act (testing)        |
// | /qa-only            | QualityAssurance (RO) | Evaluate (audit)     |
// | /retro              | EngineeringManager    | Plan (retro analysis)|
// | /cso                | SecurityOfficer       | Evaluate (security)  |
// | /design-consultation| Designer               | Clarify (UI/UX)      |
// ============================================================================

/// The cognitive role the LoopEngine should adopt during a run.
///
/// Inspired by gstack's 9 specialist slash commands — each locks the
/// model into a specific mindset with explicit constraints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[derive(Default)]
pub enum LoopRole {
    /// Pragmatic full-stack developer. Default mode.
    #[default]
    Developer,
    /// Systems architect. Design, coupling, boundaries, testability.
    Architect,
    /// Paranoid staff engineer. N+1 queries, race conditions, trust boundaries.
    Reviewer,
    /// QA lead. Test coverage, regression, reliability.
    QualityAssurance,
    /// Founder / CEO. Product vision, user empathy, "is this the right thing?".
    Founder,
    /// Release Engineer. Sync, test, push, PR creation. No more talking.
    ReleaseEngine,
    /// Security Officer. OWASP, STRIDE, trust boundary audits.
    SecurityOfficer,
    /// Engineering Manager. Retrospectives, velocity, team patterns.
    EngineeringManager,
    /// Designer. UI/UX review, visual consistency, accessibility.
    Designer,
}

impl LoopRole {
    /// System-prompt suffix that biases the LLM toward this role.
    pub fn system_prompt_suffix(&self) -> &'static str {
        match self {
            LoopRole::Developer => "You are a pragmatic full-stack developer. Ship working code.",
            LoopRole::Architect => "You are a systems architect. Think about design, coupling, maintainability, and failure modes before writing any code. Produce diagrams: sequence, state, component, data-flow.",
            LoopRole::Reviewer => "You are a paranoid staff engineer who has been burned by production incidents. Find bugs that pass CI but blow up in production. Check N+1 queries, race conditions, trust boundary violations, missing indexes, broken invariants.",
            LoopRole::QualityAssurance => "You are a QA lead. Every feature needs a test. Every edge case needs coverage. Regression is not an option. If you can't verify it, don't ship it.",
            LoopRole::Founder => "You are a founder with product taste. Don't take requests literally. Ask what the real product is. Challenge assumptions. Push back on weak ideas. Find the 10-star product hiding inside the request.",
            LoopRole::ReleaseEngine => "You are a release engineer. The branch is ready. Sync main, run tests, resolve issues, push, open PR. Handle shipping hygiene. Land the plane.",
            LoopRole::SecurityOfficer => "You are a Chief Security Officer. Audit every change through OWASP and STRIDE lenses. Check input validation, output encoding, auth flows, secret handling. A single vulnerability invalidates the entire feature.",
            LoopRole::EngineeringManager => "You are an engineering manager. Think about velocity, team patterns, technical debt, and long-term health. Balance speed with sustainability. Identify friction points.",
            LoopRole::Designer => "You are a design consultant. Check visual consistency, layout correctness, responsive behavior, color contrast, and accessibility. AI-generated UI often has subtle alignment issues — catch them.",
        }
    }

    /// Which phase this role primarily focuses on.
    pub fn primary_phase(&self) -> LoopPhase {
        match self {
            LoopRole::Founder | LoopRole::Designer => LoopPhase::Clarify,
            LoopRole::Architect | LoopRole::EngineeringManager => LoopPhase::Plan,
            LoopRole::Developer | LoopRole::ReleaseEngine => LoopPhase::Act,
            LoopRole::Reviewer | LoopRole::SecurityOfficer | LoopRole::QualityAssurance => LoopPhase::Evaluate,
        }
    }

    /// Parse from string (for CLI --role flag).
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "dev" | "developer" => Some(LoopRole::Developer),
            "arch" | "architect" => Some(LoopRole::Architect),
            "review" | "reviewer" => Some(LoopRole::Reviewer),
            "qa" | "quality-assurance" => Some(LoopRole::QualityAssurance),
            "founder" | "ceo" => Some(LoopRole::Founder),
            "release" | "ship" => Some(LoopRole::ReleaseEngine),
            "sec" | "security" | "cso" => Some(LoopRole::SecurityOfficer),
            "em" | "eng-manager" | "manager" => Some(LoopRole::EngineeringManager),
            "design" | "designer" => Some(LoopRole::Designer),
            _ => None,
        }
    }

    /// List of all valid role names for CLI help text.
    pub fn all_names() -> &'static [&'static str] {
        &[
            "developer", "architect", "reviewer", "quality-assurance",
            "founder", "release-engine", "security-officer",
            "engineering-manager", "designer",
        ]
    }
}


// ============================================================================
// Verdict and round result
// ============================================================================

/// Final verdict after a loop round.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LoopVerdict {
    /// All checks passed, loop can stop.
    Passed,
    /// Failed with a reason (may be retried).
    Failed { reason: String },
    /// Critical failure — abort immediately.
    Aborted { reason: String },
}

/// Result of a single loop round.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RoundResult {
    pub round: u32,
    pub verdict: LoopVerdict,
    pub phase_times_ms: Vec<(LoopPhase, u64)>,
    pub total_time_ms: u64,
    pub details: String,
    /// Requirement-level changes detected in this round.
    pub spec_deltas: Vec<SpecDelta>,
}

// ============================================================================
// Core traits (Observer, Planner, Actor, Evaluator)
// ============================================================================

/// A sensor that collects observations from the environment.
///
/// - Code review: [`CodeScanner`](crate::review::CodeScanner) reads source files, runs AST analysis.
/// - Browser test: [`BrowserObserver`](crate::test::BrowserObserver) captures screenshot + DOM.
#[async_trait]
pub trait Observer: Send {
    /// The type of observation this sensor produces.
    type Observation: Send + Clone;
    /// Observe the target and return observations.
    async fn observe(&mut self, target: &str) -> anyhow::Result<Self::Observation>;
    /// Describe the sensor for logging.
    fn name(&self) -> &str;
}

/// Makes decisions based on observations.
///
/// Both code review and browser test use LLM-based planning,
/// but with different context and action spaces.
#[async_trait]
pub trait Planner: Send {
    /// The type of observation this planner consumes.
    type Observation: Send;
    /// The type of plan this producer produces.
    type Plan: Send;
    /// Generate a plan from observations.
    async fn plan(&mut self, observation: &Self::Observation) -> anyhow::Result<Self::Plan>;
    fn name(&self) -> &str;
    /// Optional: inject constitution context into the planner's state
    /// before `plan()` is called. Default is a no-op.
    fn with_constitution_context(&mut self, _context: &str) {}
}

/// An actuator that executes plans.
///
/// - Code review: [`FileEditActor`](crate::review::FileEditActor) applies edits via DiffEngine.
/// - Browser test: [`PlaywrightActor`](crate::test::PlaywrightActor) clicks, types, scrolls.
#[async_trait]
pub trait Actor: Send {
    /// The type of plan this actuator consumes.
    type Plan: Send;
    /// The type of result produced by executing the plan.
    type ActionResult: Send + Clone;
    /// Execute the plan.
    async fn act(&mut self, plan: &Self::Plan) -> anyhow::Result<Self::ActionResult>;
    fn name(&self) -> &str;
    /// Optional: provide a human-readable summary of the action result.
    /// Used to populate `RoundResult.details` for reports.
    fn action_summary(&self, _result: &Self::ActionResult) -> String {
        String::new()
    }
}

/// Judges whether an action succeeded.
///
/// - Code review: [`CompilerEvaluator`](crate::review::CompilerEvaluator) runs cargo check.
/// - Browser test: [`VisualEvaluator`](crate::test::VisualEvaluator) compares screenshots + checks console.
#[async_trait]
pub trait Evaluator: Send {
    /// The type of action result this evaluator judges.
    type ActionResult: Send;
    /// Evaluate the result of an action.
    async fn evaluate(&mut self, result: &Self::ActionResult) -> anyhow::Result<LoopVerdict>;
    fn name(&self) -> &str;
}

/// A clarifying question generated during the Clarify phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClarifyQuestion {
    /// The question text.
    pub question: String,
    /// Optional predefined answer options.
    pub options: Vec<String>,
}

/// Resolves ambiguities between Observe and Plan.
///
/// Examines the observation summary (and optional constitution) to identify
/// underspecified requirements, then generates clarifying questions.
/// If no clarification is needed, returns an empty vector.
#[async_trait]
pub trait Clarifier: Send {
    /// Given an observation summary and optional constitution, produce
    /// clarifying questions. Return empty vec if nothing to clarify.
    async fn clarify(
        &mut self,
        observation_summary: &str,
        constitution_prompt: &str,
    ) -> anyhow::Result<Vec<ClarifyQuestion>>;
    fn name(&self) -> &str;
}

// ============================================================================
// LoopConfig
// ============================================================================

/// Configuration for the unified agent loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopConfig {
    /// Maximum number of loop rounds (default: 5).
    pub max_rounds: u32,
    /// Whether to print detailed per-phase timing.
    pub verbose: bool,
    /// Maximum time per round in seconds (default: 120).
    pub round_timeout_secs: u64,
    /// Role-specific mode (changes system prompts and evaluation).
    pub role: LoopRole,
    /// Whether to inject Iron Laws into the clarifier prompt.
    pub use_iron_laws: bool,
    /// Whether to enforce the Review Gate (abort if no review happened in Act phase).
    pub enforce_review_gate: bool,
    /// Ratchet mode: auto-revert (`git checkout -- .`) on failed rounds.
    /// Only changes from passed rounds survive. (autoresearch keep-or-discard pattern)
    pub ratchet_mode: bool,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            max_rounds: 5,
            verbose: false,
            round_timeout_secs: 120,
            role: LoopRole::default(),
            use_iron_laws: true,
            enforce_review_gate: false,
            ratchet_mode: false,
        }
    }
}

// ============================================================================
// LoopEngine — the generic 4-phase loop
// ============================================================================

/// Generic 4-phase loop engine.
///
/// Drives the Observe → Plan → Act → Evaluate cycle with configurable
/// sensors, planners, actuators, and evaluators.
///
/// ## Type Parameters
///
/// - `S`: Observer (sensor)
/// - `P`: Planner (decision maker)
/// - `A`: Actor (actuator)
/// - `E`: Evaluator (judge)
pub struct LoopEngine<S, P, A, E>
where
    S: Observer,
    P: Planner<Observation = S::Observation>,
    A: Actor<Plan = P::Plan>,
    E: Evaluator<ActionResult = A::ActionResult>,
{
    observer: S,
    planner: P,
    actor: A,
    evaluator: E,
    config: LoopConfig,
    round: u32,
    history: Vec<RoundResult>,
    /// Run mode label ("review" | "test") for hook events.
    mode: String,
    /// Optional hook registry for observability callbacks.
    hooks: Option<crate::hooks::HookRegistry>,
    /// Optional project constitution — injected into planner before each plan() call.
    constitution: Option<Constitution>,
    /// Optional clarifier — runs between Observe and Plan if set.
    clarifier: Option<Box<dyn Clarifier>>,
}

impl<S, P, A, E> LoopEngine<S, P, A, E>
where
    S: Observer,
    P: Planner<Observation = S::Observation>,
    A: Actor<Plan = P::Plan>,
    E: Evaluator<ActionResult = A::ActionResult>,
{
    /// Create a new loop engine.
    pub fn new(
        observer: S,
        planner: P,
        actor: A,
        evaluator: E,
        config: LoopConfig,
    ) -> Self {
        Self {
            observer,
            planner,
            actor,
            evaluator,
            config,
            round: 0,
            history: Vec::new(),
            mode: "unknown".into(),
            hooks: None,
            constitution: None,
            clarifier: None,
        }
    }

    /// Set the run mode label (e.g. "review", "test") for observability.
    pub fn with_mode(mut self, mode: impl Into<String>) -> Self {
        self.mode = mode.into();
        self
    }

    /// Attach a hook registry for lifecycle callbacks.
    pub fn with_hooks(mut self, hooks: crate::hooks::HookRegistry) -> Self {
        self.hooks = Some(hooks);
        self
    }

    /// Inject a project constitution — constraints injected into the planner's system prompt.
    pub fn with_constitution(mut self, constitution: Constitution) -> Self {
        self.constitution = Some(constitution);
        self
    }

    /// Attach an optional clarifier (runs between Observe and Plan).
    pub fn with_clarifier(mut self, clarifier: Box<dyn Clarifier>) -> Self {
        self.clarifier = Some(clarifier);
        self
    }

    /// Get the current round number (0-based).
    pub fn current_round(&self) -> u32 {
        self.round
    }

    /// Get the maximum number of rounds.
    pub fn max_rounds(&self) -> u32 {
        self.config.max_rounds
    }

    /// Get the loop execution history.
    pub fn history(&self) -> &[RoundResult] {
        &self.history
    }

    /// Run the full loop until passed, aborted, or max_rounds exhausted.
    ///
    /// Features (Phase 4 absorption):
    /// - **Fixed Budget** (autoresearch): each round is wrapped in `tokio::time::timeout`
    ///   using `config.round_timeout_secs`. Timed-out rounds become `Failed`.
    /// - **Ratchet Mode** (autoresearch): when `config.ratchet_mode=true`, failed rounds
    ///   trigger `git checkout -- .` to discard changes — only improvements survive.
    /// - **Experiment Log**: each round's result is appended to `.carp/experiments/`
    ///   as a structured JSON file for post-hoc analysis.
    pub async fn run(&mut self, target: &str) -> Vec<RoundResult> where <S as Observer>::Observation: std::fmt::Debug {
        info!(
            "LoopEngine[{}|{}|{}|{}] starting — max_rounds={}, timeout={}s, ratchet={}, target={}",
            self.observer.name(),
            self.planner.name(),
            self.actor.name(),
            self.evaluator.name(),
            self.config.max_rounds,
            self.config.round_timeout_secs,
            self.config.ratchet_mode,
            target,
        );

        // Ensure experiments directory exists
        let _ = std::fs::create_dir_all(".carp/experiments");

        loop {
            if self.round >= self.config.max_rounds {
                warn!("LoopEngine: max_rounds ({}) reached", self.config.max_rounds);
                break;
            }

            // P1-A3: Fixed Budget Evaluation — enforce per-round timeout
            let timeout_dur = std::time::Duration::from_secs(self.config.round_timeout_secs);
            let result = match tokio::time::timeout(timeout_dur, self.round_once(target)).await {
                Ok(r) => r,
                Err(_) => {
                    warn!("LoopEngine: Round {} timed out after {}s", self.round + 1, self.config.round_timeout_secs);
                    RoundResult {
                        round: self.round + 1,
                        verdict: LoopVerdict::Failed {
                            reason: format!("Round timed out after {}s", self.config.round_timeout_secs),
                        },
                        phase_times_ms: vec![],
                        total_time_ms: self.config.round_timeout_secs * 1000,
                        details: "Timeout — round exceeded budget".to_string(),
                        spec_deltas: vec![],
                    }
                }
            };

            // P1-A2: Ratchet Mode — auto-revert on failure
            if self.config.ratchet_mode
                && matches!(result.verdict, LoopVerdict::Failed { .. }) {
                    info!("LoopEngine[Ratchet]: Round {} failed — reverting changes via `git checkout -- .`", self.round + 1);
                    let _ = std::process::Command::new("git")
                        .args(["checkout", "--", "."])
                        .current_dir(target)
                        .output();
                }

            // P1-A4: Experiment Log — write structured JSON per round
            Self::log_experiment_round(self.round + 1, &result, target);

            // Fire round-completed hook
            let verdict_str = match &result.verdict {
                LoopVerdict::Passed => "Passed".into(),
                LoopVerdict::Failed { reason } => format!("Failed: {}", reason),
                LoopVerdict::Aborted { reason } => format!("Aborted: {}", reason),
            };
            if let Some(ref hooks) = self.hooks {
                hooks.fire(crate::hooks::HookEvent::LoopRoundCompleted {
                    target: target.to_string(),
                    mode: self.mode.clone(),
                    round: self.round + 1,
                    verdict: verdict_str,
                    total_time_ms: result.total_time_ms,
                }).await;
            }

            let should_stop = match &result.verdict {
                LoopVerdict::Passed => {
                    info!("LoopEngine: Round {} Passed ✅", self.round + 1);
                    true
                }
                LoopVerdict::Aborted { reason } => {
                    warn!("LoopEngine: Round {} Aborted: {}", self.round + 1, reason);
                    true
                }
                LoopVerdict::Failed { reason } => {
                    warn!("LoopEngine: Round {} Failed: {}", self.round + 1, reason);
                    false
                }
            };

            self.history.push(result);
            self.round += 1;

            if should_stop {
                break;
            }
        }

        // Fire run-completed hook
        let passed = self.history.last().is_some_and(|r| r.verdict == LoopVerdict::Passed);
        if let Some(ref hooks) = self.hooks {
            hooks.fire(crate::hooks::HookEvent::LoopRunCompleted {
                target: target.to_string(),
                mode: self.mode.clone(),
                total_rounds: self.history.len() as u32,
                passed,
                total_time_ms: self.history.iter().map(|r| r.total_time_ms).sum(),
            }).await;
        }

        info!(
            "LoopEngine: finished after {} rounds (passed={}, failed={})",
            self.history.len(),
            self.history.iter().filter(|r| r.verdict == LoopVerdict::Passed).count(),
            self.history.iter().filter(|r| matches!(r.verdict, LoopVerdict::Failed { .. })).count(),
        );

        self.history.clone()
    }

    /// Execute a single round of Observe → (Clarify?) → Plan → Act → Evaluate.
    async fn round_once(&mut self, target: &str) -> RoundResult where <S as Observer>::Observation: std::fmt::Debug {
        let round_start = Instant::now();
        let mut phase_times = Vec::new();

        // 1. Observe
        let obs_start = Instant::now();
        let observation = match self.observer.observe(target).await {
            Ok(obs) => obs,
            Err(e) => {
                let elapsed = obs_start.elapsed().as_millis() as u64;
                phase_times.push((LoopPhase::Observe, elapsed));
                return RoundResult {
                    round: self.round + 1,
                    verdict: LoopVerdict::Aborted { reason: format!("Observe failed: {}", e) },
                    phase_times_ms: phase_times,
                    total_time_ms: elapsed,
                    details: String::new(),
                    spec_deltas: Vec::new(),
                };
            }
        };
        let obs_time = obs_start.elapsed().as_millis() as u64;
        phase_times.push((LoopPhase::Observe, obs_time));

        // 1b. Clarify (optional) — between Observe and Plan
        if let Some(ref mut clarifier) = self.clarifier {
            let clarify_start = Instant::now();
            let constit_prompt = self.constitution
                .as_ref()
                .map(|c| c.to_system_prompt())
                .unwrap_or_default();

            // Build enforcement prompt: role suffix + optional Iron Laws
            let mut enforcement_prompt = String::new();
            enforcement_prompt.push_str("\n## Role\n");
            enforcement_prompt.push_str(self.config.role.system_prompt_suffix());
            enforcement_prompt.push('\n');
            if self.config.use_iron_laws {
                let enforcer = crate::rules::iron_laws::IronLawEnforcer::default();
                enforcement_prompt.push_str(&enforcer.to_system_prompt());
            }

            let full_prompt = format!("{}\n{}", constit_prompt, enforcement_prompt);
            let obs_summary = format!("{:?}", &observation);

            match clarifier.clarify(&obs_summary, &full_prompt).await {
                Ok(questions) if !questions.is_empty() => {
                    info!(
                        "Clarify[{}]: generated {} question(s)",
                        clarifier.name(),
                        questions.len()
                    );
                    // For now, log questions; in a future iteration we could
                    // collect user responses and feed them back to the planner.
                    for (i, q) in questions.iter().enumerate() {
                        info!("  Clarify Q{}: {} (options: {:?})", i + 1, q.question, q.options);
                    }
                }
                Ok(_) => { /* no clarification needed */ }
                Err(e) => {
                    // Clarify failure is non-fatal — continue to Plan
                    warn!("Clarify[{}] error (proceeding): {}", clarifier.name(), e);
                }
            }
            phase_times.push((LoopPhase::Clarify, clarify_start.elapsed().as_millis() as u64));
        }

        // 2. Plan — with optional constitution context
        if let Some(ref constitution) = self.constitution {
            let ctx = constitution.to_system_prompt();
            if !ctx.is_empty() {
                self.planner.with_constitution_context(&ctx);
            }
        }
        let plan_start = Instant::now();
        let plan = match self.planner.plan(&observation).await {
            Ok(p) => p,
            Err(e) => {
                let elapsed = plan_start.elapsed().as_millis() as u64;
                phase_times.push((LoopPhase::Plan, elapsed));
                return RoundResult {
                    round: self.round + 1,
                    verdict: LoopVerdict::Failed { reason: format!("Plan failed: {}", e) },
                    phase_times_ms: phase_times,
                    total_time_ms: plan_start.elapsed().as_millis() as u64,
                    details: String::new(),
                    spec_deltas: Vec::new(),
                };
            }
        };
        phase_times.push((LoopPhase::Plan, plan_start.elapsed().as_millis() as u64));

        // 3. Act
        let act_start = Instant::now();
        let action_result = match self.actor.act(&plan).await {
            Ok(r) => r,
            Err(e) => {
                let elapsed = act_start.elapsed().as_millis() as u64;
                phase_times.push((LoopPhase::Act, elapsed));
                return RoundResult {
                    round: self.round + 1,
                    verdict: LoopVerdict::Failed { reason: format!("Act failed: {}", e) },
                    phase_times_ms: phase_times,
                    total_time_ms: act_start.elapsed().as_millis() as u64,
                    details: String::new(),
                    spec_deltas: Vec::new(),
                };
            }
        };
        let action_details = self.actor.action_summary(&action_result);
        phase_times.push((LoopPhase::Act, act_start.elapsed().as_millis() as u64));

        // 3b. Review Gate (hard gate — inspired by gstack's /review enforcement)
        // If enforce_review_gate is true AND the action_details does not contain
        // review evidence (e.g. "reviewed", "lint", "clippy", "audit"),
        // abort the round with a hard error.
        if self.config.enforce_review_gate {
            let has_review_evidence = action_details.contains("review")
                || action_details.contains("lint")
                || action_details.contains("clippy")
                || action_details.contains("audit")
                || action_details.contains("security scan");
            if !has_review_evidence {
                warn!(
                    "ReviewGate: Round {} aborted — no review evidence in action output",
                    self.round + 1
                );
                return RoundResult {
                    round: self.round + 1,
                    verdict: LoopVerdict::Aborted {
                        reason: "ReviewGate: Action completed without review evidence. \
                            The Review Iron Law requires every change to be reviewed before \
                            evaluation. Add a review step to your plan."
                            .into(),
                    },
                    phase_times_ms: phase_times,
                    total_time_ms: act_start.elapsed().as_millis() as u64,
                    details: action_details,
                    spec_deltas: Vec::new(),
                };
            }
            info!("ReviewGate: Round {} passed review gate", self.round + 1);
        }

        // 4. Evaluate
        let eval_start = Instant::now();
        let verdict = match self.evaluator.evaluate(&action_result).await {
            Ok(v) => v,
            Err(e) => LoopVerdict::Failed { reason: format!("Evaluate error: {}", e) },
        };
        phase_times.push((LoopPhase::Evaluate, eval_start.elapsed().as_millis() as u64));

        let total = round_start.elapsed().as_millis() as u64;

        if self.config.verbose {
            info!("Round {} timing: {:?}", self.round + 1, phase_times);
        }

        // 4b. Analyze: generate spec deltas from plan vs action if successful
        let spec_deltas = if verdict == LoopVerdict::Passed {
            generate_spec_deltas(&self.mode, target, &action_details)
        } else {
            Vec::new()
        };

        RoundResult {
            round: self.round + 1,
            verdict,
            phase_times_ms: phase_times,
            total_time_ms: total,
            details: action_details,
            spec_deltas,
        }
    }

    /// Reset the loop to round 0, clearing history.
    pub fn reset(&mut self) {
        self.round = 0;
        self.history.clear();
    }

    /// Access the observer (for configuration).
    pub fn observer(&self) -> &S { &self.observer }
    /// Access the observer mutably.
    pub fn observer_mut(&mut self) -> &mut S { &mut self.observer }
    /// Access the planner.
    pub fn planner(&self) -> &P { &self.planner }
    /// Access the actor.
    pub fn actor(&self) -> &A { &self.actor }
    /// Access the evaluator.
    pub fn evaluator(&self) -> &E { &self.evaluator }
}

// ============================================================================
// Convenience: LoopResult summary
// ============================================================================

/// High-level summary of a complete loop run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopSummary {
    pub total_rounds: u32,
    pub passed: bool,
    pub final_verdict: Option<LoopVerdict>,
    pub total_time_ms: u64,
    pub results: Vec<RoundResult>,
}

impl<S, P, A, E> LoopEngine<S, P, A, E>
where
    S: Observer,
    P: Planner<Observation = S::Observation>,
    A: Actor<Plan = P::Plan>,
    E: Evaluator<ActionResult = A::ActionResult>,
{
    /// Run and summarize.
    pub async fn run_summary(&mut self, target: &str) -> LoopSummary where <S as Observer>::Observation: std::fmt::Debug {
        let start = Instant::now();
        let results = self.run(target).await;
        let total = start.elapsed().as_millis() as u64;

        let last = results.last();
        LoopSummary {
            total_rounds: results.len() as u32,
            passed: last.is_some_and(|r| r.verdict == LoopVerdict::Passed),
            total_time_ms: total,
            final_verdict: last.map(|r| r.verdict.clone()),
            results,
        }
    }

    /// P1-A4: Write a single round's result to `.carp/experiments/` as structured JSON.
    ///
    /// Each experiment log captures the full round result for post-hoc analysis
    /// (autoresearch pattern: every iteration is recorded for later review).
    fn log_experiment_round(round_num: u32, result: &RoundResult, target: &str) {
        let timestamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
        let filename = format!(".carp/experiments/round-{}-{}.json", timestamp, round_num);

        let entry = serde_json::json!({
            "round": round_num,
            "target": target,
            "verdict": match &result.verdict {
                LoopVerdict::Passed => serde_json::json!("passed"),
                LoopVerdict::Failed { reason } => serde_json::json!({"failed": reason}),
                LoopVerdict::Aborted { reason } => serde_json::json!({"aborted": reason}),
            },
            "total_time_ms": result.total_time_ms,
            "phase_times_ms": result.phase_times_ms
                .iter()
                .map(|(p, t)| (format!("{:?}", p), *t))
                .collect::<std::collections::HashMap<String, u64>>(),
            "details": result.details,
            "spec_delta_count": result.spec_deltas.len(),
            "timestamp": timestamp.to_string(),
        });

        // Best-effort write — failure to log should not abort the loop
        if let Ok(json_str) = serde_json::to_string_pretty(&entry) {
            let _ = std::fs::write(&filename, json_str);
        }
    }
}

// ============================================================================
// Analyze helper — generate spec deltas from action results
// ============================================================================

/// Generate spec deltas from a successful round's action.
///
/// Parses the action details string to produce structured delta entries.
/// Returns an empty vec when the details contain no actionable content.
fn generate_spec_deltas(mode: &str, target: &str, action_details: &str) -> Vec<SpecDelta> {
    if action_details.is_empty() {
        return Vec::new();
    }

    // Each non-empty action detail line becomes a MODIFIED delta
    let mut deltas = Vec::new();
    for line in action_details.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        deltas.push(SpecDelta::new(
            crate::rules::spec::SpecAction::Modified,
            mode,
            format!("{}: {}", target, trimmed),
        ));
    }

    if deltas.is_empty() {
        // Fallback: one generic delta
        deltas.push(SpecDelta::new(
            crate::rules::spec::SpecAction::Modified,
            mode,
            format!("{} — action applied", target),
        ));
    }

    deltas
}

// ============================================================================
// Markdown report generation
// ============================================================================

/// Generate a Markdown refactoring report from a loop run summary.
///
/// Includes target info, summary table, per-round details,
/// phase timing breakdown, and diffs (if any).
pub fn generate_markdown_report(
    target: &str,
    mode: &str,
    summary: &LoopSummary,
    diff_summary: Option<&str>,
) -> String {
    let now = chrono::Utc::now();
    let mut md = String::new();

    // Header
    md.push_str(&format!("# 🔄 Loop Report: {} mode\n\n", mode));
    md.push_str(&format!("- **Target**: `{}`\n", target));
    md.push_str(&format!("- **Date**: {}\n", now.format("%Y-%m-%d %H:%M:%S UTC")));
    md.push_str(&format!("- **Rounds**: {}\n", summary.total_rounds));
    md.push_str(&format!("- **Status**: {}\n", if summary.passed { "✅ Passed" } else { "❌ Failed" }));
    md.push_str(&format!("- **Total Time**: {} ms\n\n", summary.total_time_ms));

    // Summary table
    md.push_str("## Summary\n\n");
    md.push_str("| Metric | Value |\n");
    md.push_str("|--------|-------|\n");
    md.push_str(&format!("| Total rounds | {} |\n", summary.total_rounds));
    md.push_str(&format!("| Passed | {} |\n", summary.passed));
    md.push_str(&format!("| Total time | {} ms |\n", summary.total_time_ms));

    if let Some(ref verdict) = summary.final_verdict {
        let verdict_str = match verdict {
            LoopVerdict::Passed => "Passed".into(),
            LoopVerdict::Failed { reason } => format!("Failed: {}", reason),
            LoopVerdict::Aborted { reason } => format!("Aborted: {}", reason),
        };
        md.push_str(&format!("| Final verdict | {} |\n", verdict_str));
    }
    md.push('\n');

    // Diff summary
    if let Some(diff) = diff_summary {
        if !diff.is_empty() {
            md.push_str("## Changes Applied\n\n");
            md.push_str("```diff\n");
            md.push_str(diff);
            if !diff.ends_with('\n') {
                md.push('\n');
            }
            md.push_str("```\n\n");
        }
    }

    // Per-round details
    md.push_str("## Rounds\n\n");
    for (i, round) in summary.results.iter().enumerate() {
        let verdict_str = match &round.verdict {
            LoopVerdict::Passed => "✅ Passed".into(),
            LoopVerdict::Failed { reason } => format!("❌ Failed: {}", reason),
            LoopVerdict::Aborted { reason } => format!("⛔ Aborted: {}", reason),
        };
        md.push_str(&format!("### Round {}: {}\n\n", i + 1, verdict_str));
        md.push_str(&format!("- **Time**: {} ms\n", round.total_time_ms));

        // Phase timing
        md.push_str("- **Phase Timing**:\n");
        md.push_str("  | Phase | Time (ms) |\n");
        md.push_str("  |-------|----------|\n");
        for (phase, ms) in &round.phase_times_ms {
            md.push_str(&format!("  | {:?} | {} |\n", phase, ms));
        }
        md.push('\n');

        if !round.details.is_empty() {
            md.push_str(&format!("**Details**: {}\n\n", round.details));
        }

        // Spec deltas
        if !round.spec_deltas.is_empty() {
            md.push_str("**Spec Changes**:\n\n");
            for delta in &round.spec_deltas {
                md.push_str(&format!("{}\n", delta.to_markdown()));
            }
            md.push('\n');
        }
    }

    // Footer
    md.push_str("---\n");
    md.push_str(&format!("_Report generated by deepseek-carp `{}` mode_", mode));

    md
}

// ============================================================================
// Tests
// ============================================================================

pub mod conductor;

#[cfg(test)]
mod tests {
    use super::*;

    // ── Mock implementations for testing ──

    struct MockObserver {
        name: String,
    }

    #[async_trait]
    impl Observer for MockObserver {
        type Observation = String;
        async fn observe(&mut self, _target: &str) -> anyhow::Result<Self::Observation> {
            Ok("observed".into())
        }
        fn name(&self) -> &str { &self.name }
    }

    struct MockPlanner {
        should_fail: bool,
    }

    #[async_trait]
    impl Planner for MockPlanner {
        type Observation = String;
        type Plan = String;
        async fn plan(&mut self, obs: &Self::Observation) -> anyhow::Result<Self::Plan> {
            if self.should_fail {
                anyhow::bail!("plan failed");
            }
            Ok(format!("plan({})", obs))
        }
        fn name(&self) -> &str { "mock-planner" }
    }

    struct MockActor {
        should_fail: bool,
    }

    #[async_trait]
    impl Actor for MockActor {
        type Plan = String;
        type ActionResult = String;
        async fn act(&mut self, plan: &Self::Plan) -> anyhow::Result<Self::ActionResult> {
            if self.should_fail {
                anyhow::bail!("act failed");
            }
            Ok(format!("acted({})", plan))
        }
        fn name(&self) -> &str { "mock-actor" }
    }

    struct MockEvaluator {
        should_pass: bool,
    }

    #[async_trait]
    impl Evaluator for MockEvaluator {
        type ActionResult = String;
        async fn evaluate(&mut self, _result: &Self::ActionResult) -> anyhow::Result<LoopVerdict> {
            if self.should_pass {
                Ok(LoopVerdict::Passed)
            } else {
                Ok(LoopVerdict::Failed { reason: "mock failure".into() })
            }
        }
        fn name(&self) -> &str { "mock-evaluator" }
    }

    #[tokio::test]
    async fn test_loop_passes_in_one_round() {
        let mut engine = LoopEngine::new(
            MockObserver { name: "obs".into() },
            MockPlanner { should_fail: false },
            MockActor { should_fail: false },
            MockEvaluator { should_pass: true },
            LoopConfig { max_rounds: 5, ..Default::default() },
        );
        let results = engine.run("test-target").await;
        assert_eq!(results.len(), 1, "should pass in 1 round");
        assert_eq!(results[0].verdict, LoopVerdict::Passed);
    }

    #[tokio::test]
    async fn test_loop_retries_on_failure() {
        let mut engine = LoopEngine::new(
            MockObserver { name: "obs".into() },
            MockPlanner { should_fail: false },
            MockActor { should_fail: false },
            MockEvaluator { should_pass: false },
            LoopConfig { max_rounds: 3, ..Default::default() },
        );
        let results = engine.run("test-target").await;
        assert_eq!(results.len(), 3, "should use all 3 rounds when evaluator keeps failing");
        for r in &results {
            assert!(matches!(r.verdict, LoopVerdict::Failed { .. }));
        }
    }

    #[tokio::test]
    async fn test_loop_aborts_on_observe_failure() {
        struct FailObserver;
        #[async_trait]
        impl Observer for FailObserver {
            type Observation = String;
            async fn observe(&mut self, _target: &str) -> anyhow::Result<Self::Observation> {
                anyhow::bail!("network error");
            }
            fn name(&self) -> &str { "fail-obs" }
        }

        let mut engine = LoopEngine::new(
            FailObserver,
            MockPlanner { should_fail: false },
            MockActor { should_fail: false },
            MockEvaluator { should_pass: true },
            LoopConfig::default(),
        );
        let results = engine.run("test").await;
        assert_eq!(results.len(), 1, "should stop immediately on abort");
        assert!(matches!(results[0].verdict, LoopVerdict::Aborted { .. }));
    }

    #[tokio::test]
    async fn test_loop_reset() {
        let mut engine = LoopEngine::new(
            MockObserver { name: "obs".into() },
            MockPlanner { should_fail: false },
            MockActor { should_fail: false },
            MockEvaluator { should_pass: true },
            LoopConfig { max_rounds: 1, ..Default::default() },
        );
        engine.run("target").await;
        assert!(engine.current_round() > 0);
        engine.reset();
        assert_eq!(engine.current_round(), 0);
        assert!(engine.history().is_empty());
    }

    #[tokio::test]
    async fn test_loop_phases_cycle() {
        assert_eq!(LoopPhase::Observe.next(), Some(LoopPhase::Plan));
        assert_eq!(LoopPhase::Plan.next(), Some(LoopPhase::Act));
        assert_eq!(LoopPhase::Act.next(), Some(LoopPhase::Evaluate));
        assert_eq!(LoopPhase::Evaluate.next(), None);
    }

    #[tokio::test]
    async fn test_loop_run_summary() {
        let mut engine = LoopEngine::new(
            MockObserver { name: "obs".into() },
            MockPlanner { should_fail: false },
            MockActor { should_fail: false },
            MockEvaluator { should_pass: true },
            LoopConfig::default(),
        );
        let summary = engine.run_summary("test").await;
        assert!(summary.passed);
        assert_eq!(summary.total_rounds, 1);
    }
}