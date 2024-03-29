//! A very small and imperfect actor system.
mod channel;
mod mailbox;
pub mod network;
mod timer;

use log::{debug, trace};
use std::time::Duration;
use std::{
    any::Any,
    collections::HashMap,
    fmt::{self, Debug},
};
use std::{
    any::TypeId,
    sync::{Arc, Mutex},
};

use channel::Channel;
use mailbox::MailboxSender;

pub use channel::ChannelRef;
pub use timer::{ScheduleId, Timer};

pub trait Message: Debug + Clone + Send + 'static {}

/// A lightweight, typed reference to interact with its underlying
/// actor instance through concurrent messaging.
///
/// All ActorRefs are products of `system.actor_of`
/// or `context.actor_of`. When an actor is created using `actor_of`
/// an `ActorRef<Msg>` is returned, where `Msg` is the mailbox
/// message type for the actor.
///
/// Actor references are lightweight and can be cloned without concern
/// for memory use.
///
/// Messages sent to an actor are added to the actor's mailbox.
///
/// In the event that the underlying actor is terminated messages sent
/// to the actor will be routed to dead letters.
///
/// If an actor is restarted all existing references continue to
/// be valid.
pub struct ActorRef<A: Actor> {
    path: String,
    mailbox: MailboxSender<A>,
}

impl<A: Actor> ActorRef<A> {
    /// Sends a message unconditionally, ignoring any potential errors.
    ///
    /// The message is always queued, even if the mailbox for the receiver is full.
    /// If the mailbox is closed, the message is silently dropped.
    pub fn send_msg<M>(&self, msg: M)
    where
        M: Message,
        A: Receiver<M>,
    {
        let _ = self.mailbox.send(msg);
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// Register a Stream to the actor mailbox.
    pub fn subscribe_to_stream<S>(&self, stream: S) -> StreamHandle
    where
        S: futures::Stream + 'static + Unpin + Send,
        S::Item: Message,
        A: Receiver<S::Item>,
    {
        let receiver = self.clone();
        let handle = tokio::spawn(async move {
            use futures::StreamExt;
            let mut stream = stream;

            while let Some(msg) = stream.next().await {
                receiver.send_msg(msg);
            }
        });

        StreamHandle { handle }
    }
}

impl<A: Actor> fmt::Debug for ActorRef<A> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ActorRef[{:?}]", self.path())
    }
}
// Manually implementing Clone because we don't want the transitive Clone dependency on A
impl<A: Actor> Clone for ActorRef<A> {
    fn clone(&self) -> Self {
        ActorRef {
            path: self.path.clone(),
            mailbox: self.mailbox.clone(),
        }
    }
}

pub struct StreamHandle {
    handle: tokio::task::JoinHandle<()>,
}
impl StreamHandle {
    pub fn cancel(&self) {
        // Should we do that differently ?
        self.handle.abort()
    }
}

/// Provides context, including the actor system during actor execution.
///
/// `Context` is passed to an actor's functions, such as
/// `receive`.
///
/// Operations performed are in most cases done so from the
/// actor's perspective. For example, creating a child actor
/// using `ctx.actor_of` will create the child under the current
/// actor within the heirarchy.
///
/// Since `Context` is specific to an actor and its functions
/// it is not cloneable.
pub struct Context<A: Actor> {
    pub myself: ActorRef<A>,
    system: ActorSystem,
}

impl<A> Context<A>
where
    A: Actor,
{
    /// Create an actor under the current actor
    ///
    /// If the actor can implement [`std::default::Default`], consider using [`ActorSystem::default_actor_of`]
    pub fn actor_of<A2>(&self, name: &str, actor: A2) -> Result<ActorRef<A2>, CreateError>
    where
        A2: Actor,
    {
        self.system.actor_of(name, actor)
    }

    /// Create an actor under the current actor
    #[allow(unused)]
    pub fn default_actor_of<A2>(&self, name: &str) -> Result<ActorRef<A2>, CreateError>
    where
        A2: Actor + Default,
    {
        self.actor_of(name, A2::default())
    }

    pub fn channel<M: Message>(&self) -> ChannelRef<M> {
        self.system.channel()
    }

    /// Register a Stream to the actor mailbox.
    pub fn subscribe_to_stream<S>(&self, stream: S) -> StreamHandle
    where
        S: futures::Stream + 'static + Unpin + Send,
        S::Item: Message,
        A: Receiver<S::Item>,
    {
        self.myself.subscribe_to_stream(stream)
    }
}

impl<Ac: Actor> Timer for Context<Ac> {
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
        self.system.schedule(initial_delay, interval, receiver, msg)
    }

    fn schedule_once<A, M>(&self, delay: Duration, receiver: ActorRef<A>, msg: M) -> ScheduleId
    where
        A: Receiver<M>,
        M: Message,
    {
        self.system.schedule_once(delay, receiver, msg)
    }

    fn cancel_schedule(&self, id: ScheduleId) {
        self.system.cancel_schedule(id)
    }
}

#[derive(Debug, Clone)]
enum RootMessage {}

impl Message for RootMessage {}

/// The actor runtime and common services coordinator
///
/// The `ActorSystem` provides a runtime on which actors are executed.
/// It also provides common services such as channels and scheduling.
#[derive(Clone)]
pub struct ActorSystem {
    //proto: Arc<ProtoSystem>,
    //root: ActorRef<RootMessage>, // TODO system root actor
    //debug: bool,
    pub timer: timer::TimerRef,
    // Require this field to be Send + Sync, which all maps don't do
    // automatically unless keys and values also implements them.
    // Unfortunately that means we can block a thread. Which isn't a good
    // thing in a highly concurrent setup, but should be enough for this
    // project. Especially because this lock is only taken on channel ref
    // acquisition, which should often happens at initialization.
    // Wait and See approach though.
    // Map of topic name to ActorRef.
    actors: Arc<Mutex<HashMap<String, Box<dyn Any + Send>>>>,
    channels: Arc<Mutex<HashMap<TypeId, Box<dyn Any + Send>>>>,
}

/// Error type when an actor fails to start during `actor_of`.
#[derive(Debug)]
pub enum CreateError {
    AlreadyExists(String),
}

impl std::fmt::Display for CreateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CreateError::AlreadyExists(name) => {
                write!(f, "Actor `{}` already exists in the system", name)
            }
        }
    }
}

impl std::error::Error for CreateError {}

impl ActorSystem {
    pub fn new() -> ActorSystem {
        let timer = timer::TokioTimer::new();
        let actors = Arc::new(Mutex::new(HashMap::new()));
        let channels = Arc::new(Mutex::new(HashMap::new()));

        ActorSystem {
            timer,
            actors,
            channels,
        }
    }

    /// Create an actor under the system root
    ///
    /// If the actor can implement [`std::default::Default`], consider using [`ActorSystem::default_actor_of`]
    pub fn actor_of<A, S>(&self, name: S, actor: A) -> Result<ActorRef<A>, CreateError>
    where
        A: Actor,
        S: Into<String>,
    {
        let name = name.into();
        trace!("actor_of::taking lock on self.actors");
        let mut actors = self.actors.lock().unwrap();
        trace!("actor_of::lock taken on self.actors");

        let entry = actors.entry(name);
        let path = entry.key().clone();

        // Check if the actor doesn't already exists
        match entry {
            std::collections::hash_map::Entry::Occupied(_) => {
                return Err(CreateError::AlreadyExists(path))
            }
            _ => (),
        };

        trace!("Creating actor at path {}", path);
        let (sender, rx) = mailbox::mailbox();
        let addr = ActorRef {
            path: path.clone(),
            mailbox: sender,
        };

        let context = Context {
            myself: addr.clone(),
            system: self.clone(),
        };
        let mut actor = actor;

        // We have an actor so we save its reference into self.actors.
        // Note that it is still not running but because actor.pre_start
        // often needs to create their own actors, we need to release our
        // lock on self.actors before invoking it.
        drop(actors);

        debug!("[{}] pre_start", path);
        actor.pre_start(&context);

        trace!("spawning actor");
        // create the fiber
        // TODO Do we want to keep track of the JoinHandle ?
        // TODO We might need some kind of system side channel, to be able to terminate actors for example
        // TODO This is also where we might want some kind of supervision
        let _handle = tokio::spawn(async move {
            // Those assignment aren't strictly needed, but they make it easier to see what
            // was moved inside the actor run loop.
            let actor_path = path;
            let mut mailbox = rx;
            let mut actor = actor;
            let context = context;

            debug!("Actor {} running mail loop", actor_path);

            while let Some(mut envelope) = mailbox.recv().await {
                trace!("[{}] received message", actor_path);
                envelope.process_message(&context, &mut actor);
            }

            actor.post_stop(&context);
            trace!("Actor {} has stopped", actor_path)
        });

        trace!("returning address {:?}", addr);
        Ok(addr)
    }

    /// Create an actor under the system root
    pub fn default_actor_of<A>(&self, name: &str) -> Result<ActorRef<A>, CreateError>
    where
        A: Actor + Default,
    {
        self.actor_of(name, A::default())
    }

    /// Get access to a channel for a given message type
    fn channel<M: Message>(&self) -> ChannelRef<M> {
        trace!("find or create channel");
        let mut channels = self.channels.lock().unwrap();
        let type_id = TypeId::of::<M>();

        trace!("Looking up channel for type id {:?}", type_id);
        match channels.get(&type_id) {
            Some(channel) => {
                trace!("Channel found");
                if let Some(addr) = channel.downcast_ref::<ChannelRef<M>>() {
                    let r = addr.clone();
                    debug!("Channel {:?} found", r);

                    r
                } else {
                    // error instead ?
                    panic!("The topic message type differ from the one it was registered with")
                }
            }
            None => {
                trace!("No channel found, creating channel");
                // Create channel for given type
                // - What's the actor name ? /system/channels/type_id ?
                let actor = self
                    .default_actor_of::<Channel<M>>(&format!("channels/{:?}", type_id))
                    .unwrap();

                debug!("Channel actor created at {:?}", actor);
                let c_ref = ChannelRef::new(actor);
                channels.insert(type_id, Box::new(c_ref.clone()));

                trace!("Channel actor stored in channels, returning ref and releasing lock");
                c_ref
            }
        }
    }
}

impl fmt::Debug for ActorSystem {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "ActorSystem[Name: {}, Start Time: {}, Uptime: {} seconds]",
            "self.name()", "self.start_date()", "self.uptime()"
        )
    }
}

impl Timer for ActorSystem {
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
        self.timer.schedule(initial_delay, interval, receiver, msg)
    }

    fn schedule_once<A, M>(&self, delay: Duration, receiver: ActorRef<A>, msg: M) -> ScheduleId
    where
        A: Receiver<M>,
        M: Message,
    {
        self.timer.schedule_once(delay, receiver, msg)
    }

    fn cancel_schedule(&self, id: ScheduleId) {
        self.timer.cancel_schedule(id)
    }
}

/// Actors are objects which encapsulate state and behavior.
///
/// This trait only manage an actor lifecycle events. To be
/// able to receive messages, the [`Receiver`] trait needs
/// to be implemented for each message type.
pub trait Actor: Sized + Send + 'static {
    /// Invoked when an actor is being started by the system.
    ///
    /// Any initialization inherent to the actor's role should be
    /// performed here.
    ///
    /// Panics in `pre_start` do not invoke the
    /// supervision strategy and the actor will be terminated.
    fn pre_start(&mut self, _ctx: &Context<Self>) {}

    /// Invoked after an actor has been stopped. Note that sending
    /// messages to the `myself` [`ActorRef`] in the given context
    /// will lead to their loss, as the actor is now stopped.
    /// The [`Context`] is passed as a mean to do clean up operations.
    fn post_stop(&mut self, _ctx: &Context<Self>) {}
}

/// Describes how to handle messages of a specific type.
///
/// Implementing `Handler` is a general way to handle incoming
/// messages, streams, and futures.
///
/// The type `M` is a message which can be handled by the actor.
pub trait Receiver<M>
where
    Self: Actor,
    M: Message,
{
    /// Invoked when an actor receives a message
    ///
    /// It is guaranteed that only one message in the actor's mailbox is processed
    /// at any one time, including `recv` and `sys_recv`.
    fn recv(&mut self, ctx: &Context<Self>, msg: M);
}
