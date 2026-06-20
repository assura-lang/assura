mod agent;
mod audit;
mod build;
mod check;
mod cli;
mod coverage;
mod diff;
mod fmt_cmd;
mod infer;
mod init;
mod ir_cmd;
mod ir_prompt_cmd;
mod legacy;
mod lsp_doctor;
mod repl;
mod shared;
mod test_gen;
mod timing;

use assura_config::{CompilerConfig, OutputMode, Verbosity};
use assura_parser::ast::*;
use assura_parser::lexer::Token;
use logos::Logos;
use std::fs;
use std::path::Path;
use std::process;
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub(crate) use agent::*;
pub(crate) use audit::*;
pub(crate) use build::*;
pub(crate) use check::*;
pub(crate) use coverage::*;
pub(crate) use diff::*;
pub(crate) use fmt_cmd::*;
pub(crate) use infer::*;
pub(crate) use init::*;
pub(crate) use ir_cmd::*;
pub(crate) use legacy::*;
pub(crate) use lsp_doctor::*;
pub(crate) use repl::*;
pub(crate) use shared::*;
pub(crate) use test_gen::*;

pub fn run() {
    cli::run();
}
