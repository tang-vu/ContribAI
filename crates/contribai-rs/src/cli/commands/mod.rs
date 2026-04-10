//! CLI command handlers — each subcommand has its own module.

pub mod analyze;
pub mod cache_clear;
pub mod cache_stats;
pub mod circuit_breaker;
pub mod cleanup;
pub mod config;
pub mod doctor;
pub mod dream;
pub mod encrypt_token;
pub mod hunt;
pub mod init;
pub mod leaderboard;
pub mod login;
pub mod mcp_server;
pub mod models;
pub mod notify_test;
pub mod patrol;
pub mod profile;
pub mod run;
pub mod schedule;
pub mod serve;
// pub mod session; // TODO: wire Session command when feature is complete
pub mod solve;
pub mod stats;
pub mod status;
pub mod system_status;
pub mod target;
pub mod templates;
pub mod undo;
pub mod watchlist;
pub mod web_server;
