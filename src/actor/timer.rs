use log::trace;
use std::{collections::HashMap, time::Duration};
use tokio::{sync::mpsc, task::JoinHandle, time::Instant};
use uuid::Uuid;

use super::{ActorRef, Message, Receiver};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScheduleId(Uuid);

pub trait Timer {
    fn schedule<A, M>(
        &self,
        initial_delay: Duration,
        interval: Duration,
        receiver: ActorRef<A>,
        msg: M,
    ) -> ScheduleId
    where
        A: Receiver<M>,
        M: Message;

    fn schedule_once<A, M>(&self, delay: Duration, receiver: ActorRef<A>, msg: M) -> ScheduleId
    where
        A: Receiver<M>,
        M: Message;

    fn cancel_schedule(&self, id: ScheduleId);
}

enum Job {
    //Once{},
    Interval {
        id: ScheduleId,
        initial_delay: Duration,
        interval: Duration,
        send: Box<dyn Fn() -> () + Send>,
    },
    Cancel {
        id: ScheduleId,
    },
}

pub struct TokioTimer {
    intervals: HashMap<ScheduleId, JoinHandle<()>>,
}

impl TokioTimer {
    pub fn new() -> TimerRef {
        let (tx, mut rx) = mpsc::channel(100);

        // timer run loop
        tokio::spawn(async move {
            let mut timer = TokioTimer {
                intervals: HashMap::new(),
            };

            while let Some(job) = rx.recv().await {
                match job {
                    Job::Interval {
                        id,
                        initial_delay,
                        interval,
                        send,
                    } => {
                        let start = Instant::now() + initial_delay;
                        let mut interval = tokio::time::interval_at(start, interval);
                        trace!("Received new interval job");

                        let handle = tokio::spawn(async move {
                            loop {
                                let _ = interval.tick().await;

                                trace!("Tick on job {:?}", id);
                                send();
                            }
                        });

                        timer.intervals.insert(id, handle);
                    }
                    Job::Cancel { id } => {
                        trace!("Cancelling job {:?}", id);
                        let mut aborted = 0;

                        if let Some((_, handle)) = timer.intervals.iter().find(|(i, _)| i == &&id) {
                            handle.abort();
                            aborted += 1;
                        }
                    }
                }
            }
        });

        TimerRef(tx)
    }
}

#[derive(Debug, Clone)]
pub struct TimerRef(mpsc::Sender<Job>);

impl Timer for TimerRef {
    fn schedule<A, M>(
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
        let id = ScheduleId(uuid::Uuid::new_v4());
        let job = Job::Interval {
            id,
            initial_delay,
            interval,
            send: Box::new(move || {
                receiver.send_msg(msg.clone());
            }),
        };

        let _ = self.0.try_send(job); // TODO error

        id
    }

    fn schedule_once<A, M>(&self, delay: Duration, receiver: ActorRef<A>, msg: M) -> ScheduleId
    where
        A: Receiver<M>,
        M: Message,
    {
        todo!()
    }

    fn cancel_schedule(&self, id: ScheduleId) {
        let _ = self.0.try_send(Job::Cancel { id });
    }
}
