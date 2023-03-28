use std::{
    cell::UnsafeCell,
    fmt::{self, Debug},
    ops::{Deref, DerefMut},
    ptr::NonNull,
    sync::Mutex,
};

use windows::Win32::System::Threading::{
    AcquireSRWLockExclusive, InitializeSRWLock, ReleaseSRWLockExclusive, ReleaseSRWLockShared,
    TryAcquireSRWLockExclusive, TryAcquireSRWLockShared, RTL_SRWLOCK, AcquireSRWLockShared,
};

#[cfg(test)]
mod tests;

/// A Reader-Writer Lock, see [`std::sync::RwLock`] for more information
pub struct RwLock<T: ?Sized> {
    lock: RwLockInner,
    data: UnsafeCell<T>,
}

// SAFETY: This is safe because of invariants
unsafe impl<T: Send> Send for RwLock<T> {}
// SAFETY: This is safe because of invariants
unsafe impl<T: Send + Sync> Sync for RwLock<T> {}

struct RwLockInner {
    lock: UnsafeCell<RTL_SRWLOCK>,
    state: Mutex<RwLockInnerState>,
}

impl Drop for RwLockInner {
    fn drop(&mut self) {
        println!("RwLockInner Dropped!");
    }
}

#[derive(Debug)]
enum RwLockInnerState {
    Unlocked,
    Write(std::thread::ThreadId),
    Read(Vec<std::thread::ThreadId>),
}

impl RwLockInnerState {
    fn read_acquired(&mut self) {
        match self {
            RwLockInnerState::Unlocked => {
                *self = RwLockInnerState::Read(vec![std::thread::current().id()])
            }
            RwLockInnerState::Write(_) => unreachable!(),
            RwLockInnerState::Read(v) => v.push(std::thread::current().id()),
        }
    }
    fn write_acquired(&mut self) {
        match self {
            RwLockInnerState::Unlocked => {
                *self = RwLockInnerState::Write(std::thread::current().id())
            }
            _ => unreachable!(),
        }
    }
    fn end_read(&mut self) {
        match self {
            RwLockInnerState::Read(v) => {
                let index = v
                    .iter()
                    .position(|id| id == &std::thread::current().id())
                    .unwrap();
                v.swap_remove(index);
                if v.is_empty() {
                    *self = RwLockInnerState::Unlocked;
                }
            }
            _ => unreachable!(),
        }
    }

    fn end_write(&mut self) {
        match self {
            RwLockInnerState::Write(id) => {
                assert_eq!(id, &std::thread::current().id());
                *self = RwLockInnerState::Unlocked;
            }
            _ => unreachable!(),
        }
    }
}

impl RwLockInner {
    fn new() -> Self {
        Self {
            // SAFETY: This is safe because nothing is unsafe
            lock: UnsafeCell::new(unsafe { InitializeSRWLock() }),
            state: Mutex::new(RwLockInnerState::Unlocked),
        }
    }

    /// Returns `true` if lock acquired, false if lock not acquired
    fn try_read(&self) -> bool {
        // SAFETY: This is safe because we won't move the SRW lock while it is used.
        let res = unsafe { TryAcquireSRWLockShared(self.lock.get()) }.0 != 0;

        if res {
            let mut guard = self.state.lock().unwrap();
            guard.read_acquired();
        }

        res
    }

    fn read(&self) {
        // SAFETY: This is safe because we won't move the SRW lock while it is used.
        unsafe { AcquireSRWLockShared(self.lock.get()) };
        self.state.lock().unwrap().read_acquired();
    }

    fn end_read(&self) {
        self.state.lock().unwrap().end_read();
        // SAFETY: This is safe because it is a SRW lock.
        unsafe { ReleaseSRWLockShared(self.lock.get()) };
    }

    /// Returns `true` if lock acquired, false if lock not acquired
    fn try_write(&self) -> bool {
        // SAFETY: This is safe because we won't move the SRW lock while it is used.
        let ret = unsafe { TryAcquireSRWLockExclusive(self.lock.get()) }.0 != 0;

        if ret {
            self.state.lock().unwrap().write_acquired();
        }

        ret
    }

    /// Blocks until the lock is acquired
    fn write(&self) {
        // SAFETY: This is safe because we won't move the SRW lock while it is used.
        unsafe { AcquireSRWLockExclusive(self.lock.get()) }
        self.state.lock().unwrap().write_acquired();
    }

    fn end_write(&self) {
        self.state.lock().unwrap().end_write();
        // SAFETY: This is safe because it is a SRW lock.
        unsafe { ReleaseSRWLockExclusive(self.lock.get()) }
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
    /// Returns the data stored in the RwLock.
    pub fn into_inner(self) -> T {
        self.data.into_inner()
    }
}
impl<T: ?Sized> RwLock<T> {
    /// Try to acquire a read lock.
    pub fn try_read(&self) -> Option<RwLockReadGuard<T>> {
        self.lock.try_read().then(|| {
            RwLockReadGuard {
                // SAFETY: This is safe because the pointer comes from an unsafe cell, and the SRW is locked.
                data: unsafe { NonNull::new_unchecked(self.data.get()) },
                lock: &self.lock,
            }
        })
    }

    /// Try to acquire a write lock.
    pub fn try_write(&self) -> Option<RwLockWriteGuard<T>> {
        if self.lock.try_write() {
            Some(RwLockWriteGuard { lock: self })
        } else {
            None
        }
    }

    /// Acquire a write lock
    pub fn write(&self) -> RwLockWriteGuard<T> {
        self.lock.write();
        RwLockWriteGuard { lock: self }
    }

    /// Acquire a read lock
    pub fn read(&self) -> RwLockReadGuard<T> {
        self.lock.read();
        RwLockReadGuard {
            // SAFETY: This is safe because the pointer comes from an unsafe cell, and the SRW is locked.
            data: unsafe { NonNull::new_unchecked(self.data.get()) },
            lock: &self.lock,
        }
    }

    /// Returns a mutable refernce to the data
    pub fn get_mut(&mut self) -> &mut T {
        self.data.get_mut()
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

/// A RAII read guard
#[must_use = "if unused the RwLock will immediately unlock"]
#[clippy::has_significant_drop]
pub struct RwLockReadGuard<'lock, T: ?Sized> {
    data: NonNull<T>,
    lock: &'lock RwLockInner,
}

impl<T: ?Sized> Deref for RwLockReadGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: This is safe because the SRW lock is locked, and the data is correct.
        unsafe { self.data.as_ref() }
    }
}

impl<T: ?Sized> Drop for RwLockReadGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.end_read()
    }
}

impl<T> Debug for RwLockReadGuard<'_, T>
where
    T: Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (**self).fmt(f)
    }
}

/// A RAII write guard
#[must_use = "if unused the RwLock will immediately unlock"]
#[clippy::has_significant_drop]
pub struct RwLockWriteGuard<'lock, T: ?Sized> {
    lock: &'lock RwLock<T>,
}

impl<T: ?Sized> Deref for RwLockWriteGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: This is safe because the SRW lock is locked, and the data is correct.
        unsafe { &*self.lock.data.get() }
    }
}

impl<T: ?Sized> DerefMut for RwLockWriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: This is safe because the SRW lock is locked, and the data is correct.
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<T: ?Sized> Drop for RwLockWriteGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.lock.end_write();
    }
}

impl<T> Debug for RwLockWriteGuard<'_, T>
where
    T: Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (**self).fmt(f)
    }
}
