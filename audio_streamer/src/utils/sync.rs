#[cfg(loom)]
pub use loom::sync::{Arc, mpsc};
#[cfg(loom)]
pub use loom::sync::atomic::{AtomicUsize, Ordering};
#[cfg(loom)]
pub use loom::thread;
#[cfg(loom)]
pub use loom::model;
#[cfg(loom)]
pub(crate) use loom::cell;

#[cfg(not(loom))]
pub use std::sync::{Arc, mpsc};
#[cfg(not(loom))]
pub use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(not(loom))]
pub use std::thread;
#[cfg(not(loom))]
// Identity function for non-loom builds. 
pub fn model<F: FnOnce()>(f: F) { f() }
#[cfg(not(loom))]
pub(crate) use crate::utils::cell;