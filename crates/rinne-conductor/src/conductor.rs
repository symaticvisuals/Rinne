//! The Conductor: prompt → JSON DAG, with backend fallback and a JSON-repair
//! retry (`CONTEXT.md` §7, §21).

use async_trait::async_trait;

use rinne_core::dag::Plan;
use rinne_core::replanner::Replanner;
use rinne_core::{Result, RinneError};

use crate::backend::PlanBackend;
use crate::parse::parse_plan;
use crate::prompt::{system_prompt, user_prompt, ConductorInput};

/// The conductor drives one or more backends in preference order. Each backend
/// gets one repair retry if its first output does not parse, before falling
/// through to the next backend (`CONTEXT.md` §21 graceful fallback).
pub struct Conductor {
    backends: Vec<Box<dyn PlanBackend>>,
}

impl Conductor {
    /// Build a conductor from backends in preference order (primary first).
    /// At least one backend is required.
    pub fn new(backends: Vec<Box<dyn PlanBackend>>) -> Result<Self> {
        if backends.is_empty() {
            return Err(RinneError::Conductor(
                "no conductor backend available — configure one or install a harness".into(),
            ));
        }
        Ok(Self { backends })
    }

    /// Names of the configured backends, primary first (for narration).
    pub fn backend_names(&self) -> Vec<String> {
        self.backends.iter().map(|b| b.name().to_string()).collect()
    }

    /// Produce a fresh plan from a goal and context.
    pub async fn plan(&self, input: &ConductorInput) -> Result<Plan> {
        let system = system_prompt();
        let user = user_prompt(input);
        let mut plan = self.run(&system, &user).await?;
        // Carry the @-mentioned files onto the plan deterministically. The
        // assembler inlines their contents for API workers; relying on the LLM
        // to echo the paths back in its JSON is unreliable, so set them here.
        plan.mentioned = input.mentioned.clone();
        Ok(plan)
    }

    /// Amend an existing plan given new state. For Phase 4 this re-plans from
    /// scratch with the current plan summarized into the digest; structural
    /// amendment lands with the replanner hook in Phase 5.
    pub async fn replan(&self, input: &ConductorInput) -> Result<Plan> {
        self.plan(input).await
    }

    /// Try each backend in order; within a backend, retry once with a repair
    /// nudge if the first response does not parse.
    async fn run(&self, system: &str, user: &str) -> Result<Plan> {
        let mut last_err: Option<RinneError> = None;

        for backend in &self.backends {
            match self.try_backend(backend.as_ref(), system, user).await {
                Ok(plan) => return Ok(plan),
                Err(e) => {
                    tracing::warn!("conductor backend `{}` failed: {e}", backend.name());
                    last_err = Some(e);
                }
            }
        }

        Err(last_err.unwrap_or_else(|| {
            RinneError::Conductor("all conductor backends failed".into())
        }))
    }

    async fn try_backend(
        &self,
        backend: &dyn PlanBackend,
        system: &str,
        user: &str,
    ) -> Result<Plan> {
        let raw = backend.complete(system, user).await?;
        match parse_plan(&raw) {
            Ok(plan) => Ok(finalize(plan)),
            Err(first) => {
                tracing::warn!(
                    "conductor `{}` produced unparseable plan ({first}); retrying with repair nudge",
                    backend.name()
                );
                let repair_user = format!(
                    "{user}\n\nYour previous response could not be parsed as the required JSON \
                     DAG. Return ONLY the JSON object, with no prose, comments, or code fence."
                );
                let raw2 = backend.complete(system, &repair_user).await?;
                parse_plan(&raw2).map(finalize)
            }
        }
    }
}

/// Normalize a freshly-parsed plan: Rinne owns budgets (via config), so a
/// model-supplied budget is discarded to avoid a too-tight `max_total_iterations`
/// killing an otherwise-healthy run.
fn finalize(mut plan: Plan) -> Plan {
    plan.budget = Default::default();
    plan
}

/// The conductor is the engine's replanner: a wrong-approach verdict or repeated
/// failure amends the DAG rather than grinding the same node (`CONTEXT.md` §12).
#[async_trait]
impl Replanner for Conductor {
    async fn replan(&self, goal: &str, digest: &str, _current: &Plan) -> Result<Plan> {
        let input = ConductorInput {
            goal: goal.to_string(),
            digest: Some(digest.to_string()),
            max_iterations_per_node: 8,
            ..Default::default()
        };
        Conductor::replan(self, &input).await
    }
}
