//! Mailbox is a wrapper around tokio's mpsc channel
//!
//! It provides a simplified interface on the sender side with some additional
//! features useful for the actor world.
use super::{Actor, Context, Message, Receiver};
use downcast_rs::Downcast;
use tokio::sync::mpsc;

pub enum SendError<T> {
    Full(T),
    Closed(T),
}

#[derive(Debug)]
pub struct MailboxSender<A: Actor> {
    sender: mpsc::Sender<Envelope<A>>,
}

impl<A: Actor> MailboxSender<A> {
    pub(super) fn send<M>(&self, msg: M) -> Result<(), SendError<M>>
    where
        A: Receiver<M>,
        M: Message,
    {
        // The error cases is a bit complex. Envelope is designed to hide the message
        // type, that's how we actually manage to have the dissocation between Actors
        // and Receivers. But because in the error case we want to return the actual
        // message, we need a way to extract it from the Envelope. We do so by having
        // the InnerEnvelope implements the Downcast trait and then do a downcast on
        // it. And because there is only one implementation, which we know but not the
        // compiler, we panic on another type than the one we expect. Note that it
        // wouldn't be safe in InnerEnvelope was public, because we wouldn't control
        // what can actually implement it.
        let envelope = Envelope::new(msg);
        self.sender.try_send(envelope).map_err(|err| match err {
            mpsc::error::TrySendError::Full(m) => {
                let b: Box<dyn InnerEnvelope<A>> = m.0;

                match b.downcast::<MessageEnvelope<M>>() {
                    Ok(m) => SendError::Full((*m).message.unwrap()),
                    Err(_) => panic!("Unexpected downcast"),
                }
            }
            mpsc::error::TrySendError::Closed(m) => {
                let b: Box<dyn InnerEnvelope<A>> = m.0;

                match b.downcast::<MessageEnvelope<M>>() {
                    Ok(m) => SendError::Closed((*m).message.unwrap()),
                    Err(_) => panic!("Unexpected downcast"),
                }
            }
        })
    }
}

// Manually implementing Clone because we don't want the transitive Clone dependency on A
impl<A: Actor> Clone for MailboxSender<A> {
    fn clone(&self) -> Self {
        MailboxSender {
            sender: self.sender.clone(),
        }
    }
}

/// Receiver end of a mailbox. Owned by the actor system.
pub struct Mailbox<A: Actor> {
    receiver: mpsc::Receiver<Envelope<A>>,
}

impl<A: Actor> Mailbox<A> {
    pub(super) async fn recv(&mut self) -> Option<Envelope<A>> {
        self.receiver.recv().await
    }
}

// TODO implement Stream for mailbox ?

pub fn mailbox<A: Actor>() -> (MailboxSender<A>, Mailbox<A>) {
    let (sender, receiver) = mpsc::channel(50); // TODO Config

    let sender = MailboxSender { sender };
    let mailbox = Mailbox { receiver };

    (sender, mailbox)
}

/// An Envelope encapsulate all messages an actor can receive.
pub(super) struct Envelope<A: Actor>(Box<dyn InnerEnvelope<A> + Send>);

impl<A: Actor> Envelope<A> {
    pub fn new<M>(message: M) -> Self
    where
        A: Receiver<M>,
        M: Message,
    {
        Envelope(Box::new(MessageEnvelope {
            message: Some(message),
        }))
    }

    pub(super) fn process_message(&mut self, ctx: &Context<A>, act: &mut A) {
        self.0.process_message(ctx, act)
    }
}

/// InnerEnvelope is a trait to let us hide the message type.
///
/// Underthehood this use dynamic dispatch to do this magic. Plus careful
/// proof that a message M can be received by an actor A (type bounds).
trait InnerEnvelope<A: Actor>: Downcast {
    fn process_message(&mut self, ctx: &Context<A>, act: &mut A);
}
downcast_rs::impl_downcast!(InnerEnvelope<A> where A: Actor);
//impl_downcast!(TraitGeneric3<T> assoc H where T: Copy, H: Clone);

struct MessageEnvelope<M> {
    // We use an option to be able to take ownership of the message
    // without having ownership of the envelope (using .take())
    message: Option<M>,
}

impl<A, M> InnerEnvelope<A> for MessageEnvelope<M>
where
    M: Message,
    A: Actor + Receiver<M>,
{
    fn process_message(&mut self, ctx: &Context<A>, act: &mut A) {
        if let Some(message) = self.message.take() {
            <A as Receiver<M>>::recv(act, ctx, message);
        }
    }
}
