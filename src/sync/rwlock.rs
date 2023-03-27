use std::{
    cell::UnsafeCell,
    fmt::{self, Debug},
    ops::Deref,
    ptr::NonNull,
    sync::RwLock as StdRwLock,
};

use windows::Win32::System::Threading::{InitializeSRWLock, TryAcquireSRWLockShared, RTL_SRWLOCK, ReleaseSRWLockShared};

/// A reader-writer lock
///
/// This type of lock allows a number of readers or at most one writer at any
/// point in time. The write portion of this lock typically allows modification
/// of the underlying data (exclusive access) and the read portion of this lock
/// typically allows for read-only access (shared access).
///
/// In comparison, a [`Mutex`] does not distinguish between readers or writers
/// that acquire the lock, therefore blocking any threads waiting for the lock to
/// become available. An `RwLock` will allow any number of readers to acquire the
/// lock as long as a writer is not holding the lock.
///
/// The priority policy of the lock is dependent on the underlying operating
/// system's implementation, and this type does not guarantee that any
/// particular policy will be used. In particular, a writer which is waiting to
/// acquire the lock in `write` might or might not block concurrent calls to
/// `read`, e.g.:
///
/// <details><summary>Potential deadlock example</summary>
///
/// ```text
/// // Thread 1             |  // Thread 2
/// let _rg = lock.read();  |
///                         |  // will block
///                         |  let _wg = lock.write();
/// // may deadlock         |
/// let _rg = lock.read();  |
/// ```
/// </details>
///
/// The type parameter `T` represents the data that this lock protects. It is
/// required that `T` satisfies [`Send`] to be shared across threads and
/// [`Sync`] to allow concurrent access through readers. The RAII guards
/// returned from the locking methods implement [`Deref`] (and [`DerefMut`]
/// for the `write` methods) to allow access to the content of the lock.
///
/// # Examples
///
/// ```
/// use custom_stdlib::sync::RwLock;
///
/// let lock = RwLock::new(5);
///
/// // many reader locks can be held at once
/// {
///     let r1 = lock.read().unwrap();
///     let r2 = lock.read().unwrap();
///     assert_eq!(*r1, 5);
///     assert_eq!(*r2, 5);
/// } // read locks are dropped at this point
///
/// // only one write lock may be held, however
/// {
///     let mut w = lock.write().unwrap();
///     *w += 1;
///     assert_eq!(*w, 6);
/// } // write lock is dropped here
/// ```
///
/// [`Mutex`]: super::Mutex
pub struct RwLock<T> {
    lock: RwLockInner,
    data: UnsafeCell<T>,
}

struct RwLockInner {
    lock: UnsafeCell<RTL_SRWLOCK>,
}

impl RwLockInner {
    fn new() -> Self {
        Self {
            // SAFETY: This is safe because nothing is unsafe
            lock: UnsafeCell::new(unsafe { InitializeSRWLock() }),
        }
    }

    /// Returns `true` if lock acquired, false if lock not acquired
    fn try_read(&self) -> bool {
        // SAFETY: This is safe because OS thakes care of synchronisation
        unsafe {
            TryAcquireSRWLockShared(self.lock.get())
        }.0 != 0
    }

    fn end_read(&self) {
        // SAFETY: This is safe because OS thakes care of synchronisation
        unsafe {ReleaseSRWLockShared(self.lock.get())};
    }
}

impl<T> RwLock<T> {
    /// Creates a new instance of an `RwLock<T>` which is unlocked.
    ///
    /// # Examples
    ///
    /// ```
    /// use custom_stdlib::sync::RwLock;
    ///
    /// let lock = RwLock::new(5);
    /// ```
    pub fn new(data: T) -> Self {
        Self {
            lock: RwLockInner::new(),
            data: UnsafeCell::new(data),
        }
    }

    fn try_read(&self) -> Option<RwLockReadGuard<T>> {
        match self.lock.try_read() {
            true => Some(RwLockReadGuard {
                // SAFETY: This is safe because the pointer comes from an unsafe cell, and the SRW is locked.
                data: unsafe { NonNull::new_unchecked(self.data.get()) },
                lock: &self.lock,
            }),
            false => None,
        }
    }
}

impl<T> Debug for RwLock<T>
where
    T: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut d = f.debug_struct("RwLock");
        match self.try_read() {
            Some(guard) => {
                d.field("data", &&*guard);
            }
            None => {
                struct LockedPlaceholder;
                impl fmt::Debug for LockedPlaceholder {
                    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                        f.write_str("<locked>")
                    }
                }
                d.field("data", &LockedPlaceholder);
            }
        }
        d.finish_non_exhaustive()
    }
}

#[must_use = "if unused the RwLock will immediately unlock"]
#[clippy::has_significant_drop]
pub struct RwLockReadGuard<'lock, T> {
    data: NonNull<T>,
    lock: &'lock RwLockInner,
}

impl<T> Deref for RwLockReadGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: This is safe because the SRW lock is locked, and the data is correct.
        unsafe {self.data.as_ref()}
    }
}

impl<T> Drop for RwLockReadGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.end_read()
    }
}
