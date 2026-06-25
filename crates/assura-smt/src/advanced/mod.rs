//! Advanced verification: prophecy variables, triggers, quantifier strategies,
//! codec dispatch, weak memory ordering, and liveness contracts.

mod codec;
mod liveness;
mod prophecy;
mod trigger;
mod weak_memory;

pub use codec::*;
pub use liveness::*;
pub use prophecy::*;
pub use trigger::*;
pub use weak_memory::*;
