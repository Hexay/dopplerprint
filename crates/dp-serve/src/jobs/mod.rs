mod store;
mod worker;

pub use store::{Job, JobForm, State, Store, now};
pub use worker::spawn_worker;

use std::collections::VecDeque;
use std::sync::{Condvar, Mutex};

/// FIFO of job ids waiting for the single worker.
#[derive(Default)]
pub struct Queue {
    ids: Mutex<VecDeque<String>>,
    ready: Condvar,
}

impl Queue {
    pub fn push(&self, id: String) {
        self.ids.lock().unwrap().push_back(id);
        self.ready.notify_one();
    }

    pub fn pop_blocking(&self) -> String {
        let mut ids = self.ids.lock().unwrap();
        loop {
            if let Some(id) = ids.pop_front() {
                return id;
            }
            ids = self.ready.wait(ids).unwrap();
        }
    }

    /// Number of jobs waiting ahead of `id`, or `None` if it isn't queued.
    pub fn ahead_of(&self, id: &str) -> Option<usize> {
        self.ids.lock().unwrap().iter().position(|q| q == id)
    }
}
