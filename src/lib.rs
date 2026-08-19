//! Core library for phig, a read-only terminal Git browser.
//!
//! The library never writes to stdout, never invokes a shell, and keeps Git
//! process execution independent from terminal rendering.

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
