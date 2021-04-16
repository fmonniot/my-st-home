//! A specialized actor for providing Publish/Subscribe capabilities.
//!

use super::{Actor, ActorRef, Context, Message, Receiver};
use std::{
    any::{Any, TypeId},
    collections::HashMap,
    marker::PhantomData,
};
use futures::channel::mpsc;
use tokio::sync::oneshot;

#[derive(Debug, Clone)]
pub struct ChannelRef<M, A>(ActorRef<A>, Topic<M>);

impl<M, A> ChannelRef<M, A>
where
    M: Message,
    A: Receiver<M>
{
    pub(super) fn new(r: ActorRef<A>, topic: Topic<M>) -> ChannelRef<M, A> {
        ChannelRef(r, topic)
    }

    pub fn publish(&self, msg: M) {
        self.0.send_msg(ChannelMsg::Publish {
            topic: self.1.clone(),
            msg,
        })
    }
}

#[derive(Debug, Clone)]
pub enum ChannelMsg<M: Message, A> {
    /// Publish message
    Publish {
        topic: Topic<M>,
        msg: M,
    },

    /// Subscribe given `ActorRef` to a topic on a channel
    Subscribe {
        topic: Topic<M>,
        actor: ActorRef<A>,
    },

    // Unsubscribe the given `ActorRef` from a topic on a channel
    Unsubscribe {
        topic: Topic<M>,
        actor: ActorRef<A>,
    },

    // Unsubscribe the given `ActorRef` from all topics on a channel
    UnsubscribeAll {
        actor: ActorRef<A>,
    },
}

impl<M: Message, A> Message for ChannelMsg<M, A> {}

// To be able to create the topic name as `const`. Let's see if that's a
// useful pattern or not really.
pub struct StaticTopic<M>(&'static str, std::marker::PhantomData<M>);

impl<M> StaticTopic<M> {
    pub const fn new(name: &'static str) -> StaticTopic<M> {
        StaticTopic(name, PhantomData)
    }
}

impl<M: Message> Into<Topic<M>> for StaticTopic<M> {
    fn into(self) -> Topic<M> {
        Topic::new(self.0)
    }
}

/// Topics allow channel subscribers to filter messages by interest
/// When publishing a message to a channel a Topic is provided.
#[derive(Clone, Debug)]
pub struct Topic<M: Message>(pub(super) String, std::marker::PhantomData<M>);

impl<M: Message> std::fmt::Display for Topic<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<M: Message> PartialEq for Topic<M> {
    fn eq(&self, other: &Topic<M>) -> bool {
        self.0 == other.0
    }
}
impl<M: Message> Eq for Topic<M> {}

impl<M: Message> std::hash::Hash for Topic<M> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state)
    }
}

impl<M: Message> Topic<M> {
    pub fn new(name: &str) -> Topic<M> {
        Topic(name.to_string(), PhantomData)
    }
}

#[derive(Debug)]
pub struct Channel<M: Message> {
    subscriptions: HashMap<Topic<M>, Vec<ActorRef<M>>>,
}

impl<M: Message> Default for Channel<M> {
    fn default() -> Self {
        Channel {
            subscriptions: HashMap::new(),
        }
    }
}

impl<M> Actor for Channel<M> where M: Message,
{

    // todo sys_recv and unsubscribe everyone
}

impl<M, A> Receiver<ChannelMsg<M, A>> for Channel<M> where M: Message, A: Actor + Receiver<M> {
    fn recv(&mut self, ctx: &Context<Self>, msg: ChannelMsg<M>) {
        match msg {
            ChannelMsg::Publish { topic, msg } => {
                if let Some(actors) = self.subscriptions.get(&topic) {
                    for actor in actors {
                        actor.send_msg(msg.clone());
                    }
                }
            }
            ChannelMsg::Subscribe { topic, actor } => {
                let entry = self.subscriptions.entry(topic).or_default();

                // TODO Maybe check if ActorRef is already in ? Use a Set instead of a Vec ?
                entry.push(actor);
            }
            ChannelMsg::Unsubscribe { topic, actor } => {
                unsubscribe(&mut self.subscriptions, &topic, &actor);
            }
            ChannelMsg::UnsubscribeAll { actor } => {
                // We need to release the borrow on self.subscribtions to be able to mutate it below
                let keys = self.subscriptions.keys().cloned().collect::<Vec<_>>();

                for topic in keys {
                    unsubscribe(&mut self.subscriptions, &topic, &actor)
                }
            }
        }
    }
}

fn unsubscribe<A, M>(
    subscriptions: &mut HashMap<Topic<M>, Vec<ActorRef<M>>>,
    topic: &Topic<M>,
    actor: &ActorRef<M>,
)
where
    A: Actor,
    M: Message,
    A: Receiver<M>,
{
    if let Some(actors) = subscriptions.get_mut(topic) {
        let p = actors.iter().position(|a| a.path() == actor.path());
        if let Some(index) = p {
            actors.remove(index);
        }
    }
}

/*
pub struct ChannelRegistry {
    channels: HashMap<String, Box<dyn Any>>
}

enum ChannelRegistryMsg {
    FindOrCreate { name: Topic, reply_to: oneshot::Sender<ChannelRef<M>> },
    // Delete
}

impl ChannelRegistry {
    // TODO What if Topic would need to have the messages type associated with it ?
    // If we can do something like `const MY_TOPIC: Topic<MyMessage> = Topic("my-message");` it would be pretty dope
    fn find_or_create<M: Message>(&mut self, name: Topic<M>) -> ChannelRef<M> {

        match self.channels.get(&name.0) {
            Some(channel) => {
                if let Some(addr) = channel.downcast_ref::<ChannelRef<M>>() {
                    let r = addr.clone();

                    r
                } else {
                    panic!("The topic message type differ from the one it was registered with")
                }
            }
            None => {
                // Create channel for given type
                // - What's the actor name ? /system/channels/type_id ?
                let actor = self.actor_of::<Channel<M>>(&format!("channels/{}", name)).unwrap();
                let c_ref = ChannelRef::new(actor);
                self.channels.insert(name.0, Box::new(c_ref.clone()));

                c_ref
            }
        }
    }
}

impl<Msg> Actor for ChannelRegistry where Msg: Message {
    type Msg = ChannelRegistryMsg<Msg>;

    fn recv(&mut self, ctx: &Context<Self::Msg>, msg: Self::Msg) {
        match msg {
            ChannelRegistryMsg::FindOrCreate { name, reply_to } => {
                let r = self.find_or_create(name);
                reply_to.try_send(r);
            }
        }
    }
}
 */
