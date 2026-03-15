// Gateway file to expose exploration tests from the exploration/ subdirectory
// These tests are for ABI15 grammar exploration and auditing

// Include the common test utilities
#[path = "common/mod.rs"]
mod common;

#[path = "exploration/abi15_exploration.rs"]
mod abi15_exploration;

// Note: abi15_exploration_common is loaded inside abi15_grammar_audit
// to avoid duplicate module warnings

#[path = "exploration/abi15_grammar_audit/mod.rs"]
mod abi15_grammar_audit;
