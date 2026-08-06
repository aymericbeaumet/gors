//! Go channel representation and synchronization operations.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use crate::GoInt;

/// A clonable Go `chan int` handle.
///
/// A missing inner allocation is Go's nil channel value. Non-nil clones share
/// the same queue, close state, and synchronization points.
#[derive(Clone, Debug, Default)]
pub struct GoChannelI64 {
    inner: Option<Arc<ChannelInner>>,
}

#[derive(Debug)]
struct ChannelInner {
    state: Mutex<ChannelState>,
    changed: Condvar,
}

#[derive(Debug)]
struct ChannelState {
    capacity: usize,
    values: VecDeque<GoInt>,
    rendezvous: Option<GoInt>,
    waiting_receivers: usize,
    closed: bool,
}

/// Construct the nil `chan int` value.
#[must_use]
pub fn go_channel_i64_nil() -> GoChannelI64 {
    GoChannelI64 { inner: None }
}

/// Construct a `chan int` with the requested buffer capacity.
#[must_use]
pub fn go_channel_i64_make(capacity: GoInt) -> GoChannelI64 {
    let capacity = usize::try_from(capacity).unwrap_or_else(|_| negative_channel_capacity());
    GoChannelI64 {
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

/// Return the number of currently queued values.
#[must_use]
pub fn go_channel_i64_len(channel: GoChannelI64) -> GoInt {
    let Some(inner) = channel.inner else {
        return 0;
    };
    let state = lock(&inner.state);
    GoInt::try_from(state.values.len()).unwrap_or_else(|_| channel_size_overflow())
}

/// Return the channel buffer capacity.
#[must_use]
pub fn go_channel_i64_cap(channel: GoChannelI64) -> GoInt {
    let Some(inner) = channel.inner else {
        return 0;
    };
    let state = lock(&inner.state);
    GoInt::try_from(state.capacity).unwrap_or_else(|_| channel_size_overflow())
}

/// Send one value, waiting until buffer space or a receiver is available.
pub fn go_channel_i64_send(channel: GoChannelI64, value: GoInt) {
    let Some(inner) = channel.inner else {
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

/// Receive one value, returning the element zero value after close and drain.
#[must_use]
pub fn go_channel_i64_receive_value(channel: GoChannelI64) -> GoInt {
    go_channel_i64_receive(channel).0
}

/// Receive one value and report whether it arrived before close and drain.
#[must_use]
pub fn go_channel_i64_receive(channel: GoChannelI64) -> (GoInt, bool) {
    let Some(inner) = channel.inner else {
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
                return (0, false);
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
            return (0, false);
        }
        state = wait(&inner.changed, state);
    }
}

/// Close a channel and wake blocked senders and receivers.
pub fn go_channel_i64_close(channel: GoChannelI64) {
    let Some(inner) = channel.inner else {
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

/// Report whether this handle is Go's nil channel value.
#[must_use]
pub fn go_channel_i64_is_nil(channel: GoChannelI64) -> bool {
    channel.inner.is_none()
}

/// Attempt a send for a `select` case without waiting for readiness.
///
/// A closed channel remains a ready send case and therefore raises the normal
/// Go panic. A nil or currently unavailable channel returns `false`.
pub fn go_channel_i64_try_send(channel: GoChannelI64, value: GoInt) -> bool {
    let Some(inner) = channel.inner else {
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

/// Attempt a receive for a `select` case without waiting for readiness.
///
/// The status is `0` when no communication is ready, `1` for a selected
/// closed-and-drained receive, and `2` when a value was received.
#[must_use]
pub fn go_channel_i64_try_receive(channel: GoChannelI64) -> (GoInt, GoInt) {
    let Some(inner) = channel.inner else {
        return (0, 0);
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
    if state.closed { (0, 1) } else { (0, 0) }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn wait<'a>(
    changed: &Condvar,
    state: MutexGuard<'a, ChannelState>,
) -> MutexGuard<'a, ChannelState> {
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
