use super::*;
use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
#[cfg(test)]
mod tests;

pub(super) type Job = Box<dyn FnOnce() + Send + 'static>;
struct Queue {
    jobs: VecDeque<Job>,
    stopped: bool,
    running: usize,
    alive: usize,
}
struct Shared {
    state: Mutex<Queue>,
    ready: Condvar,
}
pub(super) struct Pool {
    shared: Arc<Shared>,
    capacity: usize,
}
impl Pool {
    #[cfg(not(target_family = "wasm"))]
    pub fn new(limits: ServerCacheBackgroundLimits) -> Result<Self, ServerCacheError> {
        let pool = Self {
            shared: Arc::new(Shared {
                state: Mutex::new(Queue {
                    jobs: VecDeque::new(),
                    stopped: false,
                    running: 0,
                    alive: 0,
                }),
                ready: Condvar::new(),
            }),
            capacity: limits.queue,
        };
        for index in 0..limits.workers {
            let shared = Arc::clone(&pool.shared);
            std::thread::Builder::new()
                .name(format!("ruvane-cache-{index}"))
                .spawn(move || work(shared))
                .map_err(|_| ServerCacheError::Unavailable)?;
        }
        Ok(pool)
    }
    #[cfg(target_family = "wasm")]
    pub fn new(limits: ServerCacheBackgroundLimits) -> Result<Self, ServerCacheError> {
        let _ = (limits.workers, limits.queue);
        Err(ServerCacheError::Unavailable)
    }
    pub fn submit(&self, job: Job) -> Result<(), ServerCacheError> {
        let mut queue = self
            .shared
            .state
            .lock()
            .map_err(|_| ServerCacheError::Unavailable)?;
        if queue.stopped || queue.jobs.len() >= self.capacity {
            return Err(ServerCacheError::Unavailable);
        }
        queue.jobs.push_back(job);
        self.shared.ready.notify_one();
        Ok(())
    }
    pub fn stop(&self) {
        let jobs = if let Ok(mut queue) = self.shared.state.lock() {
            queue.stopped = true;
            self.shared.ready.notify_all();
            std::mem::take(&mut queue.jobs)
        } else {
            VecDeque::new()
        };
        drop(jobs);
    }
    pub fn stats(&self) -> Result<(usize, usize, usize), ServerCacheError> {
        let queue = self
            .shared
            .state
            .lock()
            .map_err(|_| ServerCacheError::Unavailable)?;
        Ok((queue.jobs.len(), queue.running, queue.alive))
    }
}
impl Drop for Pool {
    fn drop(&mut self) {
        self.stop();
    }
}
#[cfg(not(target_family = "wasm"))]
fn work(shared: Arc<Shared>) {
    {
        let Ok(mut queue) = shared.state.lock() else {
            return;
        };
        queue.alive += 1;
    }
    loop {
        let job = {
            let Ok(mut queue) = shared.state.lock() else {
                return;
            };
            while queue.jobs.is_empty() && !queue.stopped {
                let Ok(next) = shared.ready.wait(queue) else {
                    return;
                };
                queue = next;
            }
            if queue.stopped {
                queue.alive -= 1;
                return;
            }
            queue.running += 1;
            queue.jobs.pop_front().unwrap()
        };
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
        let Ok(mut queue) = shared.state.lock() else {
            return;
        };
        queue.running -= 1;
    }
}
