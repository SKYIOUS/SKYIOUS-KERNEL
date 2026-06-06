use core::{future::Future, pin::Pin, task::{Context, Poll}};
use alloc::boxed::Box;
use core::sync::atomic::{AtomicU64, Ordering};

pub mod executor;
pub mod keyboard;
pub mod scheduler;
pub mod thread;
pub mod process;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct TaskId(u64);

impl TaskId {
    fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        TaskId(NEXT_ID.fetch_add(1, Ordering::Relaxed))
    }
}

pub struct Task {
    id: TaskId,
    future: Pin<Box<dyn Future<Output = ()>>>,
}

impl Task {
    pub fn new(future: impl Future<Output = ()> + 'static) -> Task {
        Task {
            id: TaskId::new(),
            future: Box::pin(future),
        }
    }

    fn poll(&mut self, context: &mut Context) -> Poll<()> {
        self.future.as_mut().poll(context)
    }
}

/// A future that yields once to the executor then completes on the next poll.
pub struct YieldNow(bool);
impl YieldNow {
    pub fn new() -> Self { YieldNow(false) }
}
impl Future for YieldNow {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.0 { Poll::Ready(()) }
        else { self.0 = true; cx.waker().wake_by_ref(); Poll::Pending }
    }
}
