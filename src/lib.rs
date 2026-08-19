//! Core library for phig, a read-only terminal Git browser.
//!
//! Git inspection never invokes a shell and stays independent from terminal
//! rendering. Interactive mode renders only to its terminal, while explicit
//! machine modes write their documented stdout protocols. The opt-in updater
//! may invoke Homebrew or a verified installer through `/bin/sh` as documented
//! in the security and installation guides.

pub mod app;
pub mod cli;
pub mod config;
pub mod domain;
pub mod git;
pub mod inspect;
pub mod protocol;
pub mod runtime;
pub mod sanitize;
pub mod tui;
pub mod update;
