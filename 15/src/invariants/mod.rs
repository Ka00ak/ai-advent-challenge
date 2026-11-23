//! Инварианты: доменный движок и детерминированные проверки.

pub mod engine;

pub use engine::{
    InvariantAssessment, assess_request, assess_response, audit_kind, bypass_probe_cases,
    invariant_tokens, refusal_text, render_invariants_block, run_bypass_probe,
};
