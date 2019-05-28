use std::future::Future;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::Context;
use std::thread;
use std::time::Instant;

use futures::task::ArcWake;
use pin_utils::pin_mut;

use crate::{Timer, TimerHandle};

pub struct HelperThread {
    thread: Option<thread::JoinHandle<()>>,
    timer: TimerHandle,
    done: Arc<AtomicBool>,
}

impl HelperThread {
    pub fn new() -> io::Result<HelperThread> {
        let timer = Timer::new();
        let timer_handle = timer.handle();
        let done = Arc::new(AtomicBool::new(false));
        let done2 = done.clone();
        let thread = thread::Builder::new().spawn(move || run(timer, done2))?;

        Ok(HelperThread {
            thread: Some(thread),
            done,
            timer: timer_handle,
        })
    }

    pub fn handle(&self) -> TimerHandle {
        self.timer.clone()
    }

    pub fn forget(mut self) {
        self.thread.take();
    }
}

impl Drop for HelperThread {
    fn drop(&mut self) {
        let thread = match self.thread.take() {
            Some(thread) => thread,
            None => return,
        };
        self.done.store(true, Ordering::SeqCst);
        thread.thread().unpark();
        drop(thread.join());
    }
}

fn run(timer: Timer, done: Arc<AtomicBool>) {
    let mut waker = ArcWake::into_waker(Arc::new(CurrentThreadWaker {
        thread: thread::current(),
    }));
    let mut cx = Context::from_waker(&mut waker);

    pin_mut!(timer);
    while !done.load(Ordering::SeqCst) {
        drop(timer.as_mut().poll(&mut cx));

        timer.advance();
        match timer.next_event() {
            // Ok, block for the specified time
            Some(when) => {
                let now = Instant::now();
                if now < when {
                    thread::park_timeout(when - now)
                } else {
                    // .. continue...
                }
            }

            // Just wait for one of our futures to wake up
            None => thread::park(),
        }
    }
}

struct CurrentThreadWaker {
    thread: thread::Thread,
}

impl ArcWake for CurrentThreadWaker {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        arc_self.thread.unpark()
    }
}
