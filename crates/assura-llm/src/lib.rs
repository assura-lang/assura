//! LLM provider abstraction and contract analysis for Assura.
//!
//! Provides:
//! - `LlmProvider` trait for pluggable LLM backends
//! - `HttpProvider` for OpenAI/Anthropic-compatible APIs
//! - `MockProvider` for testing
//! - `ContractDatabase` for cross-function contract propagation
//! - Cache layer keyed by content hash
//! - Fuzzer crash artifact parsing and crash-guided contract suggestion

pub mod cache;
pub mod contract_db;
pub mod fuzz;
pub mod lemma;
pub mod prompt;
pub mod provider;
pub mod suggest;
pub mod types;

pub use contract_db::ContractDatabase;
pub use fuzz::{CrashArtifact, CrashKind, StackFrame, StackTrace};
pub use provider::{HttpProvider, LlmProvider, MockProvider};
pub use types::*;
