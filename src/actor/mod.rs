//! A very small and imperfect actor system.
mod timer;

use futures::channel::oneshot;
use std::fmt::{self, Debug};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use timer::{ScheduleId, Timer};

pub trait Message: Debug + Clone + Send + 'static {}

#[derive(Clone, Debug)]
pub enum SystemMsg {
    ActorInit,
    Command(SystemCmd),
    Event(SystemEvent),
    Failed(BasicActorRef),
}

#[derive(Clone, Debug)]
pub enum SystemCmd {
    Stop,
    Restart,
}

#[derive(Clone, Debug)]
pub enum SystemEvent {
    /// An actor was started
    ActorCreated(BasicActorRef),

    /// An actor was restarted
    ActorRestarted(BasicActorRef),

    /// An actor was terminated
    ActorTerminated(BasicActorRef),
}

/// A lightweight, un-typed reference to interact with its underlying
/// actor instance through concurrent messaging.
///
/// `BasicActorRef` can be derived from an original `ActorRef<Msg>`.
///
/// `BasicActorRef` allows for un-typed messaging using `try_tell`,
/// that will return a `Result`. If the message type was not supported,
/// the result will contain an `Error`.
///
/// `BasicActorRef` can be used when the original `ActorRef` isn't available,
/// when you need to use collections to store references from different actor
/// types, or when using actor selections to message parts of the actor hierarchy.
///
/// In general, it is better to use `ActorRef` where possible.
#[derive(Clone)]
pub struct BasicActorRef {
    pub cell: ActorCell,
}

impl BasicActorRef {
    /// Send a message to this actor
    ///
    /// Returns a result. If the message type is not supported Error is returned.
    pub fn try_tell<Msg>(&self, msg: Msg)
    // -> Result<(), AnyEnqueueError>
    where
        Msg: Message + Send,
    {
        //self.try_tell_any(&mut AnyMessage::new(msg, true))
        todo!()
    }
}

impl fmt::Debug for BasicActorRef {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "BasicActorRef[{:?}]", "self.cell.uri()")
    }
}

#[derive(Clone)]
pub struct ActorCell {
    //inner: Arc<ActorCellInner>,
}

#[derive(Clone)]
pub struct ExtendedCell<Msg: Message> {
    cell: ActorCell,
    mailbox: mpsc::Sender<Msg>,
}

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
    pub cell: ExtendedCell<Msg>,
}

impl<Msg: Message> ActorRef<Msg> {
    #[doc(hidden)]
    pub fn new(cell: ExtendedCell<Msg>) -> ActorRef<Msg> {
        ActorRef { cell }
    }

    pub fn send_msg(&self, msg: Msg) {
        // consume the result (we don't return it to user)
        //let _ = self.cell.send_msg(msg);
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
    pub system: ActorSystem,
    pub(crate) kernel: KernelRef,
}

#[derive(Clone)]
pub struct KernelRef {
    pub tx: mpsc::Sender<KernelMsg>,
}

#[derive(Debug)]
pub enum KernelMsg {
    TerminateActor,
    RestartActor,
    RunActor,
    Sys(ActorSystem),
}

/// The actor runtime and common services coordinator
///
/// The `ActorSystem` provides a runtime on which actors are executed.
/// It also provides common services such as channels and scheduling.
pub struct ActorSystem {
    //proto: Arc<ProtoSystem>,
    //sys_actors: Option<SysActors>,
    //log: LoggingSystem,
    //debug: bool,
    //pub exec: ThreadPool,
    pub timer: timer::TimerRef,
    //pub sys_channels: Option<SysChannels>,
    //pub(crate) provider: Provider,
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
        self.timer.schedule(initial_delay, interval, receiver, msg).await
    }

    async fn schedule_once<T, M>(&self, delay: Duration, receiver: ActorRef<M>, msg: T) -> ScheduleId
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

    /// Invoked when an actor receives a system message
    ///
    /// It is guaranteed that only one message in the actor's mailbox is processed
    /// at any one time, including `recv` and `sys_recv`.
    fn sys_recv(&mut self, ctx: &Context<Self::Msg>, msg: SystemMsg) {}

    /// Invoked when an actor receives a message
    ///
    /// It is guaranteed that only one message in the actor's mailbox is processed
    /// at any one time, including `recv` and `sys_recv`.
    fn recv(&mut self, ctx: &Context<Self::Msg>, msg: Self::Msg);
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Sensors {
        channel: Option<ActorRef<u32>>, // type is probably wrong
    }
    enum SensorMessage {
        ReadRequest,
    }

    impl Actor for Sensors {
        type Msg = SensorMessage;

        fn pre_start(&mut self, ctx: &Context<Self::Msg>) {
            // register to a channel as a producer
            let producer = ctx.system.channel_producer("");
            // send sensor read message
            ctx.myself.send_msg(SensorMessage::ReadRequest)
        }

        fn recv(&mut self, ctx: &Context<Self::Msg>, msg: Self::Msg) {
            match msg {
                SensorMessage::ReadRequest => {
                    let read = self.read();
                    if let Some(channel) = self.channel {
                        channel.send_msg(read);
                    }
                    let delay = Duration::from_secs(5);
                    ctx.system
                        .schedule_once(delay, ctx.myself, SensorMessage::ReadRequest);
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
