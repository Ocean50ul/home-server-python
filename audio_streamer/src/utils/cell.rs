use std::cell::UnsafeCell as StdUnsafeCell;

#[derive(Debug)]
pub(crate) struct UnsafeCell<T>(StdUnsafeCell<T>);

// This is an unsafe newtype, so we need to tell Rust it's Send/Sync
// if the inner T is. This is safe because it's just a wrapper.
unsafe impl<T: Send> Send for UnsafeCell<T> {}
unsafe impl<T: Send + Sync> Sync for UnsafeCell<T> {}

impl<T> UnsafeCell<T> {
    pub(crate) fn new(data: T) -> UnsafeCell<T> {
        UnsafeCell(StdUnsafeCell::new(data))
    }

    pub(crate) fn with_mut<R>(&self, f: impl FnOnce(*mut T) -> R) -> R {
        f(self.0.get())
    }
}