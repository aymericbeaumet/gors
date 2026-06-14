use std::cell::RefCell;

thread_local! {
    static PANIC_RETURNS_THROUGH_DEFER: RefCell<bool> = const { RefCell::new(false) };
}

pub(super) struct PanicReturnsThroughDeferGuard {
    previous: bool,
}

impl PanicReturnsThroughDeferGuard {
    pub(super) fn set(current: bool) -> Self {
        let previous = PANIC_RETURNS_THROUGH_DEFER.with(|mode| {
            let previous = *mode.borrow();
            *mode.borrow_mut() = current;
            previous
        });
        Self { previous }
    }
}

impl Drop for PanicReturnsThroughDeferGuard {
    fn drop(&mut self) {
        PANIC_RETURNS_THROUGH_DEFER.with(|mode| {
            *mode.borrow_mut() = self.previous;
        });
    }
}

pub(super) fn panic_returns_through_defer() -> bool {
    PANIC_RETURNS_THROUGH_DEFER.with(|mode| *mode.borrow())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panic_returns_guard_restores_previous_mode() {
        assert!(!panic_returns_through_defer());
        {
            let _outer = PanicReturnsThroughDeferGuard::set(true);
            assert!(panic_returns_through_defer());
            {
                let _inner = PanicReturnsThroughDeferGuard::set(false);
                assert!(!panic_returns_through_defer());
            }
            assert!(panic_returns_through_defer());
        }
        assert!(!panic_returns_through_defer());
    }
}
