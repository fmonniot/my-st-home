//! A specialized actor for providing Publish/Subscribe capabilities.
//!

use super::{Actor, ActorRef, Context, Message, Receiver};
use std::{any, collections::HashMap, convert::From, sync::Arc};

#[derive(Debug, Clone)]
pub struct ChannelRef<M: Message>(ActorRef<Channel<M>>);

impl<M> ChannelRef<M>
where
    M: Message,
{
    pub(super) fn new(r: ActorRef<Channel<M>>) -> ChannelRef<M> {
        ChannelRef(r)
    }

    pub fn publish<T>(&self, msg: M, topic: T)
    where
        T: Into<Topic>,
    {
        self.0.send_msg(ChannelMsg::Publish {
            topic: topic.into(),
            msg,
        })
    }

    pub fn subscribe_to<A, T>(&self, actor: ActorRef<A>, topic: T)
    where
        A: Receiver<M>,
        T: Into<Topic>,
    {
        let path = actor.path().to_string();
        let b = Box::new(move |m: M| {
            actor.send_msg(m.clone());
        });
        self.0.send_msg(ChannelMsg::Subscribe {
            topic: topic.into(),
            path,
            send: Arc::new(b),
        })
    }

    #[allow(unused)]
    pub fn unsuscribe_from<A>(&self, actor: &ActorRef<A>, topic: Topic)
    where
        A: Receiver<M>,
    {
        self.0.send_msg(ChannelMsg::Unsubscribe {
            topic,
            path: actor.path().to_string(),
        })
    }

    #[allow(unused)]
    pub fn unsuscribe_all<A>(&self, actor: &ActorRef<A>)
    where
        A: Receiver<M>,
    {
        self.0.send_msg(ChannelMsg::UnsubscribeAll {
            path: actor.path().to_string(),
        })
    }
}

/// SendMessage hide away how we deliver the message to the underlying actor.
/// This let us remove a dependency on an [`Actor`] type. It does have a few
/// requirement because it has to fit in a [`Message`].
type SendMessage<M> = Arc<Box<dyn Fn(M) -> () + Send + Sync>>;

#[derive(Clone)]
pub enum ChannelMsg<M: Message> {
    /// Publish message
    Publish {
        topic: Topic,
        msg: M,
    },

    /// Subscribe given actor path to a topic on a channel
    Subscribe {
        topic: Topic,
        path: String,
        send: SendMessage<M>,
    },

    // Unsubscribe the given actor path from a topic on a channel
    Unsubscribe {
        topic: Topic,
        path: String,
    },

    // Unsubscribe the given actor path from all topics on a channel
    UnsubscribeAll {
        path: String,
    },
}

// Custom implementation because we can't derive `SendMessage`
// TODO Do we want to simplify here by making SendMessage a newtype instead of alias ?
impl<M: Message> std::fmt::Debug for ChannelMsg<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ChannelMsg::Publish { topic, msg } => f
                .debug_struct("Publish")
                .field("topic", topic)
                .field("msg", msg)
                .finish(),
            ChannelMsg::Subscribe { topic, path, .. } => f
                .debug_struct("Subscribe")
                .field("topic", topic)
                .field("path", path)
                .field("send", &"fn")
                .finish(),
            ChannelMsg::Unsubscribe { topic, path } => f
                .debug_struct("Unsubscribe")
                .field("topic", topic)
                .field("path", path)
                .finish(),
            ChannelMsg::UnsubscribeAll { path } => f
                .debug_struct("UnsubscribeAll")
                .field("path", path)
                .finish(),
        }
    }
}

impl<M: Message> Message for ChannelMsg<M> {}

/// Topics allow channel subscribers to filter messages by interest
/// When publishing a message to a channel a Topic is provided.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Topic(String);

impl std::fmt::Display for Topic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// Note to me, chained from/into is pretty powerful :)
impl<S: Into<String>> From<S> for Topic {
    fn from(string: S) -> Self {
        let s = string.into();
        Topic(s)
    }
}

// TODO After migration the app to actors entirely, revisit if topic name
// make sense or if a type-based approach is enough.
pub struct Channel<M: Message> {
    subscriptions: HashMap<Topic, Vec<(String, SendMessage<M>)>>,
}

impl<M: Message> Default for Channel<M> {
    fn default() -> Self {
        Channel {
            subscriptions: HashMap::new(),
        }
    }
}

impl<M: Message> std::fmt::Debug for Channel<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("Channel")
            .field("message_type", &any::type_name::<M>())
            .field("subscriptions", &self.subscriptions.keys())
            .finish()
    }
}

impl<M> Actor for Channel<M>
where
    M: Message,
{
    // todo post_stop and unsubscribe everyone
}

impl<M> Receiver<ChannelMsg<M>> for Channel<M>
where
    M: Message,
{
    fn recv(&mut self, _ctx: &Context<Self>, msg: ChannelMsg<M>) {
        match msg {
            ChannelMsg::Publish { topic, msg } => {
                if let Some(subs) = self.subscriptions.get(&topic) {
                    for (_, send) in subs {
                        send(msg.clone())
                    }
                }
            }
            ChannelMsg::Subscribe { topic, path, send } => {
                let entry = self.subscriptions.entry(topic).or_default();

                // TODO Maybe check if path is already in ? Use a Set instead of a Vec ?
                entry.push((path, send));
            }
            ChannelMsg::Unsubscribe { topic, path } => {
                unsubscribe(&mut self.subscriptions, &topic, &path);
            }
            ChannelMsg::UnsubscribeAll { path } => {
                // We need to release the borrow on self.subscribtions to be able to mutate it below
                let keys = self.subscriptions.keys().cloned().collect::<Vec<_>>();

                for topic in keys {
                    unsubscribe(&mut self.subscriptions, &topic, &path)
                }
            }
        }
    }
}

fn unsubscribe<M>(
    subscriptions: &mut HashMap<Topic, Vec<(String, SendMessage<M>)>>,
    topic: &Topic,
    path: &String,
) where
    M: Message,
{
    if let Some(actors) = subscriptions.get_mut(topic) {
        let p = actors.iter().position(|(p, _)| p == path);
        if let Some(index) = p {
            actors.remove(index);
        }
    }
}
