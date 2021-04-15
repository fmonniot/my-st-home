//! A very small and imperfect actor system.
mod channel;
mod mailbox;
mod timer;

use channel::{Channel, ChannelRef, Topic};
use mailbox::MailboxSender;

use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{
    any::Any,
    collections::HashMap,
    fmt::{self, Debug},
};
use timer::{ScheduleId, Timer};
use tokio::sync::mpsc;

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
#[derive(Clone)]
pub struct ActorRef<Msg: Message> {
    path: String,
    mailbox: MailboxSender<Msg>,
}

impl<Msg: Message> ActorRef<Msg> {
    pub fn send_msg(&self, msg: Msg) {
        // consume the result (we don't return it to user)
        //let _ = self.cell.send_msg(msg);
    }

    pub fn path(&self) -> &str {
        todo!()
    }
}

impl<Msg: Message> fmt::Debug for ActorRef<Msg> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ActorRef[{:?}]", self.path())
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
pub struct Context<Msg: Message> {
    pub myself: ActorRef<Msg>,
    system: ActorSystem,
}

impl<Msg> Context<Msg>
where
    Msg: Message,
{
    /// Create an actor under the current actor
    ///
    /// If the actor can implement [`std::default::Default`], consider using [`ActorSystem::default_actor_of`]
    fn actor_of<A>(
        &self,
        name: &str,
        actor: A,
    ) -> Result<ActorRef<<A as Actor>::Msg>, () /* CreateError */>
    where
        A: Actor,
    {
        /*
        self.system.provider.create_actor(
            Props::new::<A>(),
            name,
            &self.myself().into(),
            &self.system,
        )
         */
        todo!()
    }

    /// Create an actor under the current actor
    fn default_actor_of<A>(&self, name: &str) -> Result<ActorRef<<A as Actor>::Msg>, ()>
    where
        A: Actor + Default,
    {
        self.actor_of(name, A::default())
    }

    /// Find, or create if none exists, a channel for the given message type and name
    fn channel<M: Message>(&self, name: Topic<M>) -> ChannelRef<M> {
        self.system.channel(name)
    }
}

#[derive(Debug, Clone)]
enum RootMessage {}

impl Message for RootMessage {}

/// The actor runtime and common services coordinator
///
/// The `ActorSystem` provides a runtime on which actors are executed.
/// It also provides common services such as channels and scheduling.
pub struct ActorSystem {
    //proto: Arc<ProtoSystem>,
    //root: ActorRef<RootMessage>, // TODO system root actor
    //log: LoggingSystem,
    //debug: bool,
    //pub exec: ThreadPool,
    pub timer: timer::TimerRef,
    // Require this field to be Send + Sync, which all maps don't do
    // automatically unless keys and values also implements them.
    // Unfortunately that means we can block a thread. Which isn't a good
    // thing in a highly concurrent setup, but should be enough for this
    // project. Especially because this lock is only taken on channel ref
    // acquisition, which should often happens at initialization.
    // Wait and See approach though.
    // Map of topic name to actor. TODO Does that actually make sense ?
    channels: Arc<Mutex<HashMap<String, Box<dyn Any + Send>>>>, //pub(crate) provider: Provider,
}

impl ActorSystem {
    /// Create an actor under the system root
    ///
    /// If the actor can implement [`std::default::Default`], consider using [`ActorSystem::default_actor_of`]
    fn actor_of<A>(
        &self,
        name: &str,
        actor: A,
    ) -> Result<ActorRef<<A as Actor>::Msg>, () /* CreateError */>
    where
        A: Actor,
    {
        /*
        self.system.provider.create_actor(
            Props::new::<A>(),
            name,
            &self.myself().into(),
            &self.system,
        )
         */
        todo!()
    }

    /// Create an actor under the system root
    fn default_actor_of<A>(&self, name: &str) -> Result<ActorRef<<A as Actor>::Msg>, ()>
    where
        A: Actor + Default,
    {
        self.actor_of(name, A::default())
    }

    fn channel<M: Message>(&self, name: Topic<M>) -> ChannelRef<M> {
        let mut channels = self.channels.lock().unwrap();

        match channels.get(&name.0) {
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
                let actor = self
                    .default_actor_of::<Channel<M>>(&format!("channels/{}", name))
                    .unwrap();
                let c_ref = ChannelRef::new(actor, name.clone());
                channels.insert(name.0, Box::new(c_ref.clone()));

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

#[async_trait::async_trait]
impl Timer for ActorSystem {
    async fn schedule<T, M>(
        &self,
        initial_delay: Duration,
        interval: Duration,
        receiver: ActorRef<M>,
        msg: T,
    ) -> ScheduleId
    where
        T: Message + Into<M>,
        M: Message,
    {
        self.timer
            .schedule(initial_delay, interval, receiver, msg)
            .await
    }

    async fn schedule_once<T, M>(
        &self,
        delay: Duration,
        receiver: ActorRef<M>,
        msg: T,
    ) -> ScheduleId
    where
        T: Message + Into<M>,
        M: Message,
    {
        self.timer.schedule_once(delay, receiver, msg).await
    }

    async fn cancel_schedule(&self, id: ScheduleId) -> bool {
        self.timer.cancel_schedule(id).await
    }
}

// TODO Once the minimum set of feature is defined, let's removed unused operations
pub trait Actor: Send + 'static {
    type Msg: Message;

    /// Invoked when an actor is being started by the system.
    ///
    /// Any initialization inherent to the actor's role should be
    /// performed here.
    ///
    /// Panics in `pre_start` do not invoke the
    /// supervision strategy and the actor will be terminated.
    fn pre_start(&mut self, ctx: &Context<Self::Msg>) {}

    /// Invoked after an actor has started.
    ///
    /// Any post initialization can be performed here, such as writing
    /// to a log file, emmitting metrics.
    ///
    /// Panics in `post_start` follow the supervision strategy.
    fn post_start(&mut self, ctx: &Context<Self::Msg>) {}

    /// Invoked after an actor has been stopped.
    fn post_stop(&mut self) {}

    /// Invoked when an actor receives a message
    ///
    /// It is guaranteed that only one message in the actor's mailbox is processed
    /// at any one time, including `recv` and `sys_recv`.
    fn recv(&mut self, ctx: &Context<Self::Msg>, msg: Self::Msg);
}

#[cfg(test)]
mod tests {
    use super::*;
    use channel::StaticTopic;

    const SENSOR_MEASUREMENT_TOPIC: StaticTopic<u32> = StaticTopic::new("sensors");

    struct Sensors {
        channel: Option<ChannelRef<u32>>, // type is probably wrong
    }

    #[derive(Debug, Clone)]
    enum SensorMessage {
        ReadRequest,
    }

    impl Message for SensorMessage {}
    impl Message for u32 {}

    impl Actor for Sensors {
        type Msg = SensorMessage;

        fn pre_start(&mut self, ctx: &Context<Self::Msg>) {
            // register to a channel as a producer
            self.channel = Some(ctx.channel(SENSOR_MEASUREMENT_TOPIC.into()));

            // send sensor read message
            ctx.myself.send_msg(SensorMessage::ReadRequest)
        }

        fn recv(&mut self, ctx: &Context<Self::Msg>, msg: Self::Msg) {
            match msg {
                SensorMessage::ReadRequest => {
                    let read = self.read();
                    if let Some(channel) = &self.channel {
                        channel.publish(read);
                    }
                    let delay = Duration::from_secs(5);
                    ctx.system
                        .schedule_once(delay, ctx.myself.clone(), SensorMessage::ReadRequest);
                }
            }
        }
    }

    impl Sensors {
        fn read(&self) -> u32 {
            0
        }
    }

    #[test]
    fn testing_my_use_case() {
        //let system = ActorSystem::new().unwrap();
    }
}
