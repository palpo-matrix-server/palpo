use std::collections::BTreeSet;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::{sync::Arc};
use std::fmt::{self, Display, Debug, Formatter};

use crate::core::Seqnum;

#[derive(Debug, Default)]
pub struct SeqnumQueue {
    queue: Arc<std::sync::Mutex<BTreeSet<Seqnum>>>,
}

pub struct SeqnumQueueFuture {
    queue: Arc<std::sync::Mutex<BTreeSet<Seqnum>>>,
    value: Seqnum,
}
impl Future for SeqnumQueueFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let queue = self.queue.lock().expect("locked");

        if let Some(first) = queue.first() {
            if first > &self.value {
                Poll::Ready(())
            } else {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        } else {
            Poll::Ready(())
        }
    }
}

pub struct SeqnumQueueGuard {
    queue: Arc<std::sync::Mutex<BTreeSet<Seqnum>>>,
    value: Seqnum,
}

impl SeqnumQueue {
    pub fn new() -> Self {
        Self {
            queue: Default::default(),
        }
    }

    pub fn push(&self, sn: Seqnum) -> SeqnumQueueGuard {
        let mut queue = self.queue.lock().expect("locked");

        queue.insert(sn);
        SeqnumQueueGuard {
            queue: Arc::clone(&self.queue),
            value: sn,
        }
    }

    pub fn reach(&self, sn: Seqnum) -> SeqnumQueueFuture {
        SeqnumQueueFuture {
            queue: Arc::clone(&self.queue),
            value: sn,
        }
    }

    pub fn contains(&self, sn: Seqnum) -> bool {
        self.queue.lock().expect("locked").contains(&sn)
    }

    pub fn is_empty(&self) -> bool {
        self.queue.lock().expect("locked").is_empty()
    }

    pub fn len(&self) -> usize {
        self.queue.lock().expect("locked").len()
    }
}

impl Drop for SeqnumQueueGuard {
    fn drop(&mut self) {
        let mut queue = self.queue.lock().expect("locked");
        queue.remove(&self.value);
    }
}

impl Display for SeqnumQueueGuard {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "SeqnumQueueGuard({})", self.value)
    }
}

impl Debug for SeqnumQueueGuard {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "SeqnumQueueGuard({})", self.value)
    }
}