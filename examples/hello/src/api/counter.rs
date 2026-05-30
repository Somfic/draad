use crate::AppContext;

use draad::{api, events};
use std::sync::atomic::Ordering;

/// A running counter, demoing mutable state + server-pushed events.
#[api(namespace = "counter")]
pub trait CounterApi {
    /// Current value.
    async fn current(&self) -> i32;

    /// Increment by one and return the new value. Also publishes a
    /// `counter/changed` event so subscribers can update in real time.
    async fn increment(&self) -> i32;
}

#[api]
impl CounterApi for AppContext {
    async fn current(&self) -> i32 {
        self.counter.load(Ordering::Relaxed)
    }

    async fn increment(&self) -> i32 {
        let n = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
        self.events.counter.emit_changed(&n);
        n
    }
}

/// Server-pushed events for the counter namespace.
#[events(namespace = "counter")]
pub trait CounterEvents {
    /// Fired after every successful `increment`.
    fn changed(payload: i32);
}
