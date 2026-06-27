//! Format-related domain checkers.
//!
//! BinaryFormatChecker, BitLevelChecker, StringEncodingChecker,
//! ChecksumChecker, ProtocolGrammarChecker, and source-level check
//! wiring moved from `checks/format.rs`.

mod binary_format;
mod bit_level;
mod checksum;
mod codec_registry;
mod protocol_grammar;
mod string_encoding;

pub(crate) use binary_format::*;
pub(crate) use bit_level::*;
pub(crate) use checksum::*;
pub(crate) use protocol_grammar::*;
pub(crate) use string_encoding::*;

pub use codec_registry::check_codec_registry;
