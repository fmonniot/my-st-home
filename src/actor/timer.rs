use log::trace;
use std::{collections::HashMap, time::Duration};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
    time::Instant,
};

use super::{ActorRef, Message, Receiver};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScheduleId(u32);

// TODO We do not want async functions
// then we need to remove the confirmation (reply_to) and generate the responses ahead of time
#[async_trait::async_trait]
pub trait Timer {
    async fn schedule<A, M>(
        &self,
        initial_delay: Duration,
        interval: Duration,
        receiver: ActorRef<A>,
        msg: M,
    ) -> ScheduleId
    where
        A: Receiver<M>,
        M: Message;

    async fn schedule_once<A, M>(
        &self,
        delay: Duration,
        receiver: ActorRef<A>,
        msg: M,
    ) -> ScheduleId
    where
        A: Receiver<M>,
        M: Message;

    async fn cancel_schedule(&self, id: ScheduleId) -> bool;
}

enum Job {
    //Once{},
    Interval {
        initial_delay: Duration,
        interval: Duration,
        send: Box<dyn Fn() -> () + Send>,
        reply_to: oneshot::Sender<ScheduleId>,
    },
    Cancel {
        id: ScheduleId,
        reply_to: oneshot::Sender<u32>,
    },
}

pub struct TokioTimer {
    intervals: HashMap<ScheduleId, JoinHandle<()>>,
    next_schedule_id: u32,
}

impl TokioTimer {
    pub fn new() -> TimerRef {
        let (tx, mut rx) = mpsc::channel(10);

        // timer run loop
        tokio::spawn(async move {
            let mut timer = TokioTimer {
                intervals: HashMap::new(),
                next_schedule_id: 0,
            };

            while let Some(job) = rx.recv().await {
                match job {
                    Job::Interval {
                        initial_delay,
                        interval,
                        send,
                        reply_to,
                    } => {
                        let start = Instant::now() + initial_delay;
                        let mut interval = tokio::time::interval_at(start, interval);
                        trace!("Received new interval job");

                        let handle = tokio::spawn(async move {
                            loop {
                                let _ = interval.tick().await;

                                send();
                            }
                        });

                        let id = timer.next_schedule_id();
                        timer.intervals.insert(id, handle);

                        reply_to.send(id); // TODO Error handling
                    }
                    Job::Cancel { id, reply_to } => {
                        trace!("Cancelling job {:?}", id);
                        let mut aborted = 0;
                        if let Some((_, handle)) = timer.intervals.iter().find(|(i, _)| i == &&id) {
                            handle.abort();
                            aborted += 1;
                        }

                        reply_to.send(aborted); // TODO Error handling
                    }
                }
            }
        });

        TimerRef(tx)
    }

    fn next_schedule_id(&mut self) -> ScheduleId {
        let next = self.next_schedule_id;
        self.next_schedule_id += 1; // TODO Manage overflow

        ScheduleId(next)
    }
}

#[derive(Debug, Clone)]
pub struct TimerRef(mpsc::Sender<Job>);

#[async_trait::async_trait]
impl Timer for TimerRef {
    async fn schedule<A, M>(
        &self,
        initial_delay: Duration,
        interval: Duration,
        receiver: ActorRef<A>,
        msg: M,
    ) -> ScheduleId
    where
        A: Receiver<M>,
        M: Message,
    {
        trace!("Creating a schedule for actor {:?}", receiver);
        let (tx, rx) = oneshot::channel();
        let job = Job::Interval {
            initial_delay,
            interval,
            send: Box::new(move || {
                receiver.send_msg(msg.clone());
            }),
            reply_to: tx,
        };

        self.0.send(job).await; // todo error

        rx.await.unwrap() // todo error
    }

    async fn schedule_once<A, M>(
        &self,
        delay: Duration,
        receiver: ActorRef<A>,
        msg: M,
    ) -> ScheduleId
    where
        A: Receiver<M>,
        M: Message,
    {
        todo!()
    }

    async fn cancel_schedule(&self, id: ScheduleId) -> bool {
        let (reply_to, rx) = oneshot::channel();

        self.0.send(Job::Cancel { id, reply_to }).await;

        match rx.await {
            Ok(0) => false,
            Ok(1) => true,
            Ok(n) => {
                panic!("Duplicated id (n={}) on job cancellation", n)
            }
            Err(err) => {
                panic!("Unhandled oneshot error: {:?}", err) // TODO error handling
            }
        }
    }
}
