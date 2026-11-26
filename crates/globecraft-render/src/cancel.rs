//! Abandoning a frame nobody will see: a still frame that a gesture overtook. The
//! frame worker gives each job a flag (`with_flag`); the CPU's row batches and the
//! relief frame's steps look at it (`cancelled`) and skip their work once it is set.
//! On a PowerVR phone a still relief frame took 250–450 ms, and a gesture that began
//! meanwhile waited for it: the globe caught up a moment late (Julien, 2026-11-26).
use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

thread_local! {
    static FLAG: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) };
}

/// Runs `work` on this thread with `flag` as its frame's cancel flag.
pub fn with_flag<R>(flag: &Arc<AtomicBool>, work: impl FnOnce() -> R) -> R {
    struct Reset(Option<Arc<AtomicBool>>);
    impl Drop for Reset {
        fn drop(&mut self) {
            let previous = self.0.take();
            FLAG.with(|slot| *slot.borrow_mut() = previous);
        }
    }
    let _reset = Reset(FLAG.with(|slot| slot.borrow_mut().replace(flag.clone())));
    work()
}

/// This thread's frame flag, for work it hands to other threads (rayon's).
pub(crate) fn flag() -> Option<Arc<AtomicBool>> {
    FLAG.with(|slot| slot.borrow().clone())
}

/// Whether `flag` says the frame was abandoned.
pub(crate) fn set(flag: &Option<Arc<AtomicBool>>) -> bool {
    flag.as_ref()
        .is_some_and(|flag| flag.load(Ordering::Relaxed))
}

/// Whether this thread's frame was abandoned.
pub fn cancelled() -> bool {
    set(&flag())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The flag holds inside `with_flag` only, also after a panic inside it.
    #[test]
    fn the_flag_holds_for_its_work_only() {
        assert!(!cancelled());
        let flag = Arc::new(AtomicBool::new(true));
        assert!(with_flag(&flag, cancelled));
        assert!(!cancelled());
        let caught = std::panic::catch_unwind(|| with_flag(&flag, || panic!("inside")));
        assert!(caught.is_err() && !cancelled());
    }
}
