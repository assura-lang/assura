//! Contract diff and evolution verification commands + tests.

use assura_parser::ast::*;
use std::fs;
use std::process;

mod cmd;
#[cfg(test)]
mod tests;

pub(crate) use cmd::*;
