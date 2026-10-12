//! A spin lock with a guard, following *Rust Atomics and Locks*, chapter 4
//! ("Building Our Own Spin Lock").
//!
//! `lock` swaps `true` into the flag with `Acquire` ordering and spins while the old value
//! was already `true`; dropping the guard stores `false` with `Release` ordering. The
//! release store of one unlock and the acquire swap of the next lock form a happens-before
//! edge, which is what makes the writes of the previous critical section visible.

use std::cell::UnsafeCell;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};

pub struct SpinLock<T> {
    locked: AtomicBool,
    value: UnsafeCell<T>,
}

// The lock hands `&mut T` to one thread at a time, so sharing the lock only moves `T`
// between threads: `T: Send` is enough, `T: Sync` is not needed.
unsafe impl<T: Send> Sync for SpinLock<T> {}

impl<T> SpinLock<T> {
    pub const fn new(value: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            value: UnsafeCell::new(value),
        }
    }

    pub fn lock(&self) -> Guard<'_, T> {
        while self.locked.swap(true, Acquire) {
            std::hint::spin_loop();
        }
        Guard { lock: self }
    }

    pub fn into_inner(self) -> T {
        self.value.into_inner()
    }
}

/// Proof that the lock is held; unlocks on drop.
pub struct Guard<'a, T> {
    lock: &'a SpinLock<T>,
}

// The guard behaves like `&mut T`, so it may be shared only when `T` itself is `Sync`.
// (`Send` is already right: `&SpinLock<T>` is `Send` exactly when `T: Send`.)
unsafe impl<T: Sync> Sync for Guard<'_, T> {}

impl<T> Deref for Guard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        // SAFETY: the existence of this guard means we hold the lock exclusively.
        unsafe { &*self.lock.value.get() }
    }
}

impl<T> DerefMut for Guard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: the existence of this guard means we hold the lock exclusively.
        unsafe { &mut *self.lock.value.get() }
    }
}

impl<T> Drop for Guard<'_, T> {
    fn drop(&mut self) {
        self.lock.locked.store(false, Release);
    }
}

/// The same lock with every ordering weakened to `Relaxed`. This is deliberately WRONG:
/// nothing orders the plain accesses inside the critical section with the flag, so the
/// data accesses race (undefined behaviour). It exists only so the lab can show what a
/// missing happens-before edge may do on a weakly ordered CPU; never copy it.
pub struct BrokenRelaxedSpinLock<T> {
    locked: AtomicBool,
    value: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for BrokenRelaxedSpinLock<T> {}

impl<T> BrokenRelaxedSpinLock<T> {
    pub const fn new(value: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            value: UnsafeCell::new(value),
        }
    }

    /// Runs `f` on the protected value while holding the (broken) lock.
    pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        while self.locked.swap(true, Relaxed) {
            std::hint::spin_loop();
        }
        // SAFETY: none. This is the bug being demonstrated.
        let out = f(unsafe { &mut *self.value.get() });
        self.locked.store(false, Relaxed);
        out
    }

    pub fn into_inner(self) -> T {
        self.value.into_inner()
    }
}

#[cfg(test)]
mod tests {
    use super::SpinLock;

    #[test]
    fn no_increment_is_lost_under_contention() {
        const THREADS: usize = 4;
        const ITERS: usize = 50_000;
        let lock = SpinLock::new(0usize);
        std::thread::scope(|s| {
            for _ in 0..THREADS {
                s.spawn(|| {
                    for _ in 0..ITERS {
                        *lock.lock() += 1;
                    }
                });
            }
        });
        assert_eq!(lock.into_inner(), THREADS * ITERS);
    }

    #[test]
    fn guard_drop_releases_the_lock() {
        let lock = SpinLock::new(Vec::new());
        lock.lock().push(1);
        // Would spin forever if the first guard had not unlocked on drop.
        lock.lock().push(2);
        assert_eq!(lock.into_inner(), [1, 2]);
    }
}
