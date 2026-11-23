//! Process-local admission for recurring daemon background cycles.
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

const DRAIN_TIMEOUT: Duration = Duration::from_secs(10);
const HOLD_DURATION: Duration = Duration::from_secs(120);

#[derive(Default)]
struct State {
    active: usize,
    draining: bool,
    until: Option<Instant>,
}

#[derive(Default)]
struct Gate {
    state: Mutex<State>,
    changed: Condvar,
}

impl Gate {
    fn enter(&self) -> BackgroundCycle<'_> {
        let mut state = self.state.lock().expect("background gate");
        loop {
            if state.draining {
                state = self.changed.wait(state).expect("background gate");
                continue;
            }
            let Some(until) = state.until else {
                break;
            };
            if Instant::now() >= until {
                state.until = None;
                break;
            }
            state = self
                .changed
                .wait_timeout(state, until.saturating_duration_since(Instant::now()))
                .expect("background gate")
                .0;
        }
        state.active += 1;
        BackgroundCycle(self)
    }

    fn hold(&self, drain: Duration, duration: Duration) -> Result<(), &'static str> {
        let mut state = self.state.lock().expect("background gate");
        let now = Instant::now();
        if state.draining || state.until.is_some_and(|until| until > now) {
            return Err("background hold already active");
        }
        let deadline = now + drain;
        state.until = None;
        state.draining = true;
        while state.active != 0 && Instant::now() < deadline {
            state = self
                .changed
                .wait_timeout(state, deadline.saturating_duration_since(Instant::now()))
                .expect("background gate")
                .0;
        }
        state.draining = false;
        if state.active != 0 || Instant::now() >= deadline {
            self.changed.notify_all();
            return Err("background cycles did not drain within the deadline");
        }
        state.until = Some(Instant::now() + duration);
        self.changed.notify_all();
        Ok(())
    }
}

/// Held from before a cycle opens its first descriptor through its last close.
#[must_use]
pub struct BackgroundCycle<'a>(&'a Gate);

impl Drop for BackgroundCycle<'_> {
    fn drop(&mut self) {
        let mut state = self.0.state.lock().expect("background gate");
        state.active -= 1;
        self.0.changed.notify_all();
    }
}

fn gate() -> &'static Gate {
    static GATE: OnceLock<Gate> = OnceLock::new();
    GATE.get_or_init(Gate::default)
}

pub fn enter() -> BackgroundCycle<'static> {
    gate().enter()
}

pub(crate) fn hold() -> Result<(), &'static str> {
    gate().hold(DRAIN_TIMEOUT, HOLD_DURATION)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread;

    #[test]
    fn waits_for_active_cycle_and_blocks_new_cycles_until_expiration() {
        let gate = Gate::default();
        let active = gate.enter();
        let (held_tx, held_rx) = mpsc::channel();
        thread::scope(|scope| {
            scope.spawn(|| {
                held_tx
                    .send(gate.hold(Duration::from_secs(1), Duration::from_millis(100)))
                    .unwrap()
            });
            while !gate.state.lock().unwrap().draining {
                thread::yield_now();
            }
            assert!(held_rx.try_recv().is_err());
            drop(active);
            assert_eq!(
                held_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
                Ok(())
            );
            let (entered_tx, entered_rx) = mpsc::channel();
            let gate = &gate;
            scope.spawn(move || {
                let _cycle = gate.enter();
                entered_tx.send(()).unwrap();
            });
            assert!(entered_rx.recv_timeout(Duration::from_millis(20)).is_err());
            entered_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        });
    }

    #[test]
    fn a_drain_timeout_reopens_admission() {
        let gate = Gate::default();
        let _cycle = gate.enter();
        assert!(gate
            .hold(Duration::from_millis(1), Duration::from_secs(1))
            .is_err());
        assert!(gate.state.lock().unwrap().until.is_none());
        let _next = gate.enter();
    }
}
