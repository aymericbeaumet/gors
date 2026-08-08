//! Go channel representations and synchronization operations.

use std::collections::VecDeque;
use std::fmt::Debug;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use crate::{GoInt, GoString};

#[derive(Debug)]
struct GoChannel<T> {
    inner: Option<Arc<ChannelInner<T>>>,
}

impl<T> Clone for GoChannel<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T> Default for GoChannel<T> {
    fn default() -> Self {
        Self { inner: None }
    }
}

#[derive(Debug)]
struct ChannelInner<T> {
    state: Mutex<ChannelState<T>>,
    changed: Condvar,
}

#[derive(Debug)]
struct ChannelState<T> {
    capacity: usize,
    values: VecDeque<T>,
    rendezvous: Option<T>,
    waiting_receivers: usize,
    closed: bool,
}

impl<T> GoChannel<T> {
    fn make(capacity: GoInt) -> Self {
        let capacity = usize::try_from(capacity).unwrap_or_else(|_| negative_channel_capacity());
        Self {
            inner: Some(Arc::new(ChannelInner {
                state: Mutex::new(ChannelState {
                    capacity,
                    values: VecDeque::with_capacity(capacity),
                    rendezvous: None,
                    waiting_receivers: 0,
                    closed: false,
                }),
                changed: Condvar::new(),
            })),
        }
    }

    fn len(self) -> GoInt {
        let Some(inner) = self.inner else {
            return 0;
        };
        let state = lock(&inner.state);
        GoInt::try_from(state.values.len()).unwrap_or_else(|_| channel_size_overflow())
    }

    fn cap(self) -> GoInt {
        let Some(inner) = self.inner else {
            return 0;
        };
        let state = lock(&inner.state);
        GoInt::try_from(state.capacity).unwrap_or_else(|_| channel_size_overflow())
    }

    fn send(self, value: T) {
        let Some(inner) = self.inner else {
            block_forever();
        };
        let mut state = lock(&inner.state);
        if state.capacity == 0 {
            loop {
                if state.closed {
                    send_on_closed_channel();
                }
                if state.waiting_receivers > 0 && state.rendezvous.is_none() {
                    state.rendezvous = Some(value);
                    inner.changed.notify_all();
                    while state.rendezvous.is_some() {
                        if state.closed {
                            state.rendezvous = None;
                            inner.changed.notify_all();
                            send_on_closed_channel();
                        }
                        state = wait(&inner.changed, state);
                    }
                    return;
                }
                state = wait(&inner.changed, state);
            }
        }

        while state.values.len() == state.capacity {
            if state.closed {
                send_on_closed_channel();
            }
            state = wait(&inner.changed, state);
        }
        if state.closed {
            send_on_closed_channel();
        }
        state.values.push_back(value);
        drop(state);
        inner.changed.notify_all();
    }

    fn receive(self) -> (T, bool)
    where
        T: Default,
    {
        let Some(inner) = self.inner else {
            block_forever();
        };
        let mut state = lock(&inner.state);
        if state.capacity == 0 {
            state.waiting_receivers = state.waiting_receivers.saturating_add(1);
            inner.changed.notify_all();
            loop {
                if let Some(value) = state.rendezvous.take() {
                    state.waiting_receivers = state.waiting_receivers.saturating_sub(1);
                    inner.changed.notify_all();
                    return (value, true);
                }
                if state.closed {
                    state.waiting_receivers = state.waiting_receivers.saturating_sub(1);
                    return (T::default(), false);
                }
                state = wait(&inner.changed, state);
            }
        }

        loop {
            if let Some(value) = state.values.pop_front() {
                inner.changed.notify_all();
                return (value, true);
            }
            if state.closed {
                return (T::default(), false);
            }
            state = wait(&inner.changed, state);
        }
    }

    fn close(self) {
        let Some(inner) = self.inner else {
            close_of_nil_channel();
        };
        let mut state = lock(&inner.state);
        if state.closed {
            close_of_closed_channel();
        }
        state.closed = true;
        drop(state);
        inner.changed.notify_all();
    }

    fn is_nil(&self) -> bool {
        self.inner.is_none()
    }

    fn try_send(self, value: T) -> bool {
        let Some(inner) = self.inner else {
            return false;
        };
        let mut state = lock(&inner.state);
        if state.closed {
            send_on_closed_channel();
        }
        if state.capacity == 0 {
            if state.waiting_receivers == 0 || state.rendezvous.is_some() {
                return false;
            }
            state.rendezvous = Some(value);
            drop(state);
            inner.changed.notify_all();
            return true;
        }
        if state.values.len() == state.capacity {
            return false;
        }
        state.values.push_back(value);
        drop(state);
        inner.changed.notify_all();
        true
    }

    fn try_receive(self) -> (T, GoInt)
    where
        T: Default,
    {
        let Some(inner) = self.inner else {
            return (T::default(), 0);
        };
        let mut state = lock(&inner.state);
        let value = if state.capacity == 0 {
            state.rendezvous.take()
        } else {
            state.values.pop_front()
        };
        if let Some(value) = value {
            drop(state);
            inner.changed.notify_all();
            return (value, 2);
        }
        (T::default(), i64::from(state.closed))
    }
}

macro_rules! define_channel_family {
    (
        $channel:ident,
        $value:ty,
        $nil:ident,
        $make:ident,
        $len:ident,
        $cap:ident,
        $send:ident,
        $receive_value:ident,
        $receive:ident,
        $close:ident,
        $is_nil:ident,
        $try_send:ident,
        $try_receive:ident
    ) => {
        #[derive(Clone, Debug, Default)]
        pub struct $channel(GoChannel<$value>);

        #[must_use]
        pub fn $nil() -> $channel {
            $channel(GoChannel::default())
        }

        #[must_use]
        pub fn $make(capacity: GoInt) -> $channel {
            $channel(GoChannel::make(capacity))
        }

        #[must_use]
        pub fn $len(channel: $channel) -> GoInt {
            channel.0.len()
        }

        #[must_use]
        pub fn $cap(channel: $channel) -> GoInt {
            channel.0.cap()
        }

        pub fn $send(channel: $channel, value: $value) {
            channel.0.send(value);
        }

        #[must_use]
        pub fn $receive_value(channel: $channel) -> $value {
            channel.0.receive().0
        }

        #[must_use]
        pub fn $receive(channel: $channel) -> ($value, bool) {
            channel.0.receive()
        }

        pub fn $close(channel: $channel) {
            channel.0.close();
        }

        #[must_use]
        pub fn $is_nil(channel: $channel) -> bool {
            channel.0.is_nil()
        }

        pub fn $try_send(channel: $channel, value: $value) -> bool {
            channel.0.try_send(value)
        }

        #[must_use]
        pub fn $try_receive(channel: $channel) -> ($value, GoInt) {
            channel.0.try_receive()
        }
    };
}

define_channel_family!(
    GoChannelI64,
    GoInt,
    go_channel_i64_nil,
    go_channel_i64_make,
    go_channel_i64_len,
    go_channel_i64_cap,
    go_channel_i64_send,
    go_channel_i64_receive_value,
    go_channel_i64_receive,
    go_channel_i64_close,
    go_channel_i64_is_nil,
    go_channel_i64_try_send,
    go_channel_i64_try_receive
);

define_channel_family!(
    GoChannelGoString,
    GoString,
    go_channel_go_string_nil,
    go_channel_go_string_make,
    go_channel_go_string_len,
    go_channel_go_string_cap,
    go_channel_go_string_send,
    go_channel_go_string_receive_value,
    go_channel_go_string_receive,
    go_channel_go_string_close,
    go_channel_go_string_is_nil,
    go_channel_go_string_try_send,
    go_channel_go_string_try_receive
);

define_channel_family!(
    GoChannelGoChannelI64,
    GoChannelI64,
    go_channel_go_channel_i64_nil,
    go_channel_go_channel_i64_make,
    go_channel_go_channel_i64_len,
    go_channel_go_channel_i64_cap,
    go_channel_go_channel_i64_send,
    go_channel_go_channel_i64_receive_value,
    go_channel_go_channel_i64_receive,
    go_channel_go_channel_i64_close,
    go_channel_go_channel_i64_is_nil,
    go_channel_go_channel_i64_try_send,
    go_channel_go_channel_i64_try_receive
);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn wait<'a, T>(
    changed: &Condvar,
    state: MutexGuard<'a, ChannelState<T>>,
) -> MutexGuard<'a, ChannelState<T>> {
    changed.wait(state).unwrap_or_else(PoisonError::into_inner)
}

fn block_forever() -> ! {
    let mutex = Mutex::new(());
    let changed = Condvar::new();
    let mut state = lock(&mutex);
    loop {
        state = changed.wait(state).unwrap_or_else(PoisonError::into_inner);
    }
}

fn negative_channel_capacity() -> ! {
    std::panic::resume_unwind(Box::new("makechan: size out of range"))
}

fn channel_size_overflow() -> ! {
    std::panic::resume_unwind(Box::new("channel size does not fit Go int"))
}

fn send_on_closed_channel() -> ! {
    std::panic::resume_unwind(Box::new("send on closed channel"))
}

fn close_of_nil_channel() -> ! {
    std::panic::resume_unwind(Box::new("close of nil channel"))
}

fn close_of_closed_channel() -> ! {
    std::panic::resume_unwind(Box::new("close of closed channel"))
}
