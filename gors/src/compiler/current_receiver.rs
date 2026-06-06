use std::cell::RefCell;

#[derive(Clone)]
pub(super) struct CurrentReceiver {
    pub(super) rust_name: String,
    pub(super) go_type: super::typeinfer::GoType,
}

pub(super) struct CurrentReceiverGuard {
    previous: Option<CurrentReceiver>,
}

thread_local! {
    static CURRENT_RECEIVER: RefCell<Option<CurrentReceiver>> = const { RefCell::new(None) };
}

impl CurrentReceiverGuard {
    pub(super) fn set(current: CurrentReceiver) -> Self {
        let previous = CURRENT_RECEIVER.with(|receiver| receiver.borrow_mut().replace(current));
        Self { previous }
    }

    pub(super) fn clear() -> Self {
        let previous =
            CURRENT_RECEIVER.with(|receiver| std::mem::take(&mut *receiver.borrow_mut()));
        Self { previous }
    }
}

impl Drop for CurrentReceiverGuard {
    fn drop(&mut self) {
        CURRENT_RECEIVER.with(|receiver| {
            *receiver.borrow_mut() = self.previous.clone();
        });
    }
}

pub(super) fn pointer_receiver_type_name() -> Option<String> {
    CURRENT_RECEIVER.with(|receiver| {
        let receiver = receiver.borrow();
        let receiver = receiver.as_ref()?;
        match super::resolved_go_type(&receiver.go_type) {
            super::typeinfer::GoType::Pointer(inner) => match *inner {
                super::typeinfer::GoType::Named(name) => Some(name),
                _ => None,
            },
            _ => None,
        }
    })
}

pub(super) fn rust_name() -> Option<String> {
    CURRENT_RECEIVER.with(|receiver| {
        receiver
            .borrow()
            .as_ref()
            .map(|receiver| receiver.rust_name.clone())
    })
}

pub(super) fn is_pointer_receiver() -> bool {
    CURRENT_RECEIVER.with(|receiver| {
        receiver.borrow().as_ref().is_some_and(|receiver| {
            matches!(receiver.go_type, super::typeinfer::GoType::Pointer(_))
        })
    })
}

pub(super) fn is_current_receiver_rust_name(name: &str) -> bool {
    CURRENT_RECEIVER.with(|receiver| {
        receiver
            .borrow()
            .as_ref()
            .is_some_and(|receiver| receiver.rust_name == name)
    })
}

pub(super) fn is_current_receiver(name: &str, go_type: &super::typeinfer::GoType) -> bool {
    CURRENT_RECEIVER.with(|receiver| {
        receiver
            .borrow()
            .as_ref()
            .is_some_and(|receiver| receiver.rust_name == name && receiver.go_type == *go_type)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receiver_context_restores_previous_value() {
        let _outer = CurrentReceiverGuard::set(CurrentReceiver {
            rust_name: "outer".to_string(),
            go_type: super::super::typeinfer::GoType::Named("Outer".to_string()),
        });
        assert!(is_current_receiver_rust_name("outer"));
        assert!(is_current_receiver(
            "outer",
            &super::super::typeinfer::GoType::Named("Outer".to_string())
        ));
        assert!(!is_current_receiver_rust_name("inner"));

        {
            let _inner = CurrentReceiverGuard::set(CurrentReceiver {
                rust_name: "inner".to_string(),
                go_type: super::super::typeinfer::GoType::Pointer(Box::new(
                    super::super::typeinfer::GoType::Named("Inner".to_string()),
                )),
            });
            assert!(is_current_receiver_rust_name("inner"));
            assert_eq!(pointer_receiver_type_name().as_deref(), Some("Inner"));
        }

        assert!(is_current_receiver_rust_name("outer"));
        assert_eq!(pointer_receiver_type_name(), None);
    }

    #[test]
    fn clear_temporarily_removes_receiver_context() {
        let _outer = CurrentReceiverGuard::set(CurrentReceiver {
            rust_name: "outer".to_string(),
            go_type: super::super::typeinfer::GoType::Named("Outer".to_string()),
        });
        {
            let _clear = CurrentReceiverGuard::clear();
            assert!(!is_current_receiver_rust_name("outer"));
        }
        assert!(is_current_receiver_rust_name("outer"));
    }
}
