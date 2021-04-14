//! A specialized actor for providing Publish/Subscribe capabilities.
//!

use std::{any::{Any, TypeId}, collections::HashMap};
use tokio::sync::oneshot;
use super::{ActorRef, Message, Actor, Context};

#[derive(Debug, Clone)]
pub struct ChannelRef<Msg: Message>(ActorRef<ChannelMsg<Msg>>);

impl<Msg: Message> ChannelRef<Msg> {
    pub(super) fn new(r: ActorRef<ChannelMsg<Msg>>) -> ChannelRef<Msg> {
        ChannelRef(r)
    }
}

#[derive(Debug, Clone)]
pub enum ChannelMsg<Msg: Message> {
    /// Publish message
    Publish{
        topic: Topic<Msg>,
        msg: Msg,
    },

    /// Subscribe given `ActorRef` to a topic on a channel
    Subscribe{
        topic: Topic<Msg>,
        actor: ActorRef<Msg>,
    },

    // Unsubscribe the given `ActorRef` from a topic on a channel
    Unsubscribe{
        topic: Topic<Msg>,
        actor: ActorRef<Msg>,
    },

    // Unsubscribe the given `ActorRef` from all topics on a channel
    UnsubscribeAll{
        actor: ActorRef<Msg>,
    },
}

impl<Msg: Message> Message for ChannelMsg<Msg> {}

/// Topics allow channel subscribers to filter messages by interest
/// When publishing a message to a channel a Topic is provided.
#[derive(Clone, Debug)]
pub struct Topic<Msg: Message>(pub(super) String, std::marker::PhantomData<Msg>);

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

pub struct Channel<Msg: Message> {
    subscriptions: HashMap<Topic<Msg>, Vec<ActorRef<Msg>>>
}

impl<Msg> Actor for Channel<Msg>
where
    Msg: Message,
{
    type Msg = ChannelMsg<Msg>;

    fn recv(&mut self, ctx: &Context<Self::Msg>, msg: Self::Msg) {

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

    // todo sys_recv and unsubscribe everyone
}

fn unsubscribe<Msg: Message>(subscriptions: &mut HashMap<Topic<Msg>, Vec<ActorRef<Msg>>>, topic: &Topic<Msg>, actor: &ActorRef<Msg>) {
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
