//! Advanced verification: prophecy variables, triggers, quantifier strategies,
//! codec dispatch, weak memory ordering, and liveness contracts.

mod codec;
mod liveness;
mod prophecy;
mod trigger;
mod weak_memory;

pub use codec::{CodecDispatcher, CodecEntry, DispatchResult};
pub use liveness::{LivenessChecker, LivenessKind, LivenessObligation};
pub use prophecy::{ProphecyError, ProphecyManager, ProphecyVariable};
pub use trigger::{TriggerManager, TriggerPattern};
pub use weak_memory::{MemoryAccess, MemoryOrdering, WeakMemoryChecker};
