//! Mailbox is a wrapper around tokio's mpsc channel
//!
//! It provides a simplified interface on the sender side with some additional
//! features useful for the actor world.
use std::marker::PhantomData;
use tokio::sync::mpsc;

pub enum SendError<T> {
    Full(T),
    Closed(T),
}

pub trait MailboxSenderExt<M> {
    /// Attempts to send a message on this `Sender<A>` without blocking.
    fn send(&self, msg: M) -> Result<(), SendError<M>>;

    // Consume the current sender. We could do &self with a + Clone bound too.
    fn xmap<M2, F1, F2>(self, narrow: F1, widen: F2) -> MapSender<Self, M, M2, F1, F2>
    where
        Self: Sized,
        F1: Fn(M2) -> M,
        F2: Fn(M) -> M2,
    {
        MapSender {
            sender: self,
            narrow,
            widen,
            _m1: PhantomData,
            _m2: PhantomData,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MailboxSender<Msg> {
    sender: mpsc::Sender<Msg>,
}

impl<M> MailboxSenderExt<M> for MailboxSender<M> {
    fn send(&self, msg: M) -> Result<(), SendError<M>> {
        todo!()
    }
}

pub struct MapSender<S, M1, M2, F1, F2>
where
    F1: Fn(M2) -> M1,
    F2: Fn(M1) -> M2,
    S: MailboxSenderExt<M1>,
{
    sender: S,
    narrow: F1,
    widen: F2,
    // And because apparently rust don't understand that M1 and M2 are used
    // in F1 and F2 respectively, we have to make the compiler happy somehow.
    _m1: PhantomData<M1>,
    _m2: PhantomData<M2>,
}

impl<S, M1, M2, F1, F2> MailboxSenderExt<M2> for MapSender<S, M1, M2, F1, F2>
where
    F1: Fn(M2) -> M1,
    F2: Fn(M1) -> M2,
    S: MailboxSenderExt<M1>,
{
    fn send(&self, msg: M2) -> Result<(), SendError<M2>> {
        let m1 = (self.narrow)(msg);
        let result: Result<(), SendError<M1>> = self.sender.send(m1);
        let widen = &self.widen;

        result.map_err(|e| match e {
            SendError::Closed(m1) => SendError::Closed(widen(m1)),
            SendError::Full(m1) => SendError::Full(widen(m1)),
        })
    }
}

/// Receiver end of a mailbox. Owned by the actor system.
pub struct Mailbox<Msg> {
    receiver: mpsc::Receiver<Msg>,
}

pub fn mailbox<Msg>() -> (MailboxSender<Msg>, Mailbox<Msg>) {
    todo!()
}
