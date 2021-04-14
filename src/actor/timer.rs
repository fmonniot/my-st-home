use mqtt::packet::puback;
use std::{collections::HashMap, time::Duration};
use tokio::{task::JoinHandle, time::Instant};

use super::{ActorRef, AnyMessage, BasicActorRef, Message};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScheduleId(u32);

pub trait Timer {
    fn schedule<T, M>(
        &self,
        initial_delay: Duration,
        interval: Duration,
        receiver: ActorRef<M>,
        msg: T,
    ) -> ScheduleId
    where
        T: Message + Into<M>,
        M: Message;

    fn schedule_once<T, M>(&self, delay: Duration, receiver: ActorRef<M>, msg: T) -> ScheduleId
    where
        T: Message + Into<M>,
        M: Message;

    fn cancel_schedule(&self, id: ScheduleId);
}

pub struct OneOffJob {}

pub struct IntervalJob {
    interval: Duration,
    receiver: BasicActorRef,
    msg: AnyMessage,
}

pub struct TokioTimer {
    intervals: HashMap<ScheduleId, JoinHandle<()>>,
    next_schedule_id: u32,
}

impl TokioTimer {
    fn next_schedule_id(&mut self) -> ScheduleId {
        let next = self.next_schedule_id;
        self.next_schedule_id += 1; // TODO Manage overflow

        ScheduleId(next)
    }

    fn new_interval(
        &mut self,
        initial_delay: Duration,
        interval: Duration,
        receiver: BasicActorRef,
        msg: AnyMessage,
    ) {
        let start = Instant::now() + initial_delay;
        let mut interval = tokio::time::interval_at(start, interval);

        let handle = tokio::spawn(async move {
            let mut msg = msg;
            loop {
                let _ = interval.tick().await;

                receiver.try_tell_any(&mut msg);
            }
        });

        let id = self.next_schedule_id();
        self.intervals.insert(id, handle);
    }

    fn cancel(&mut self, schedule_id: ScheduleId) {
        if let Some((_, handle)) = self.intervals.iter().find(|(id, _)| id == &&schedule_id) {
            handle.abort()
        }
    }
}
