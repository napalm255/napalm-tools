//! napalm-tools: fast, private, idempotent user-space system configuration.

#![forbid(unsafe_code)]

pub mod bundles;
pub mod cli;
pub mod config;
pub mod dotfiles;
pub mod execute;
pub mod managers;
pub mod plan;
pub mod platform;
pub mod privilege;
pub mod report;
pub mod shell;
pub mod ui;
pub mod update;
pub mod version;
