use log::{debug, error, info, warn};
use std::sync::Arc;
use tokio::sync::Mutex;

use super::{
    mqtt::{self, MqttClient},
    Command, Commands, DeviceEvent, DeviceEvents, Topic,
};
use crate::actor::{Actor, ChannelRef, Context, Message, Receiver, StreamHandle};
use crate::configuration::Configuration;

#[derive(Debug, thiserror::Error)]
pub enum CreateError {
    #[error("We only support ED25519 keys. {0} passed.")]
    UnsupportedIdentityType(String),
}

impl Message for Command {}

/// The SmartThings actor is the glue between the SmartThings platform
/// and our executable. It holds the connection to the platform, send
/// status updates to it and broadcast commands from the platform to the
/// actor system through channels.
pub struct SmartThings {
    client: Option<Arc<Mutex<MqttClient>>>,
    connection: ConnectionState,
    pending_events: Vec<DeviceEvent>,
    configuration: Configuration,
    commands: Option<ChannelRef<Command>>,
    subscriptions_handle: Option<StreamHandle>,
}

// TODO Change client implementation to not depends on Mutex. Probably
// something based on actors. Will need a actor::network::tcp in that
// case.
#[derive(PartialEq, Eq)]
enum ConnectionState {
    Disconnected,
    WaitingConnection,
    Connected,
}

// TODO Separate in two enums: one public and one private
// publish needs Connect/Publish, private contains lifecycle events
#[derive(Debug, Clone)]
pub enum Cmd {
    Connect,
    Publish(DeviceEvent), // TODO
}

#[derive(Debug, Clone)]
enum LifeCycleEvent {
    ConnectResult(Result<(), mqtt::ClientError>),
    PublishResult(Result<(), mqtt::ClientError>),
    SuscribeResult(Result<Vec<mqtt::SubscribeReturnCode>, mqtt::ClientError>),
}

impl Message for Cmd {}
impl Message for LifeCycleEvent {}

pub fn new(configuration: Configuration) -> Result<SmartThings, CreateError> {
    if configuration.onboarding.identity_type != "ED25519" {
        return Err(CreateError::UnsupportedIdentityType(
            configuration.onboarding.identity_type,
        ));
    }

    Ok(SmartThings {
        client: None,
        connection: ConnectionState::Disconnected,
        pending_events: vec![],
        configuration,
        commands: None,
        subscriptions_handle: None,
    })
}

impl Actor for SmartThings {
    fn pre_start(&mut self, ctx: &Context<Self>) {
        self.commands = Some(ctx.channel())
    }

    fn post_stop(&mut self, _ctx: &Context<Self>) {
        if let Some(handle) = &self.subscriptions_handle {
            handle.cancel();
        }
    }
}

impl Receiver<Cmd> for SmartThings {
    fn recv(&mut self, ctx: &Context<Self>, msg: Cmd) {
        match msg {
            Cmd::Connect => {
                let jwt = super::jwt::generate(&self.configuration).unwrap();
                let mqtt_client =
                    MqttClient::builder("mqtt-regional-useast1.api.smartthings.com", 8883)
                        .set_client_id("mqtt-console-rs")
                        .set_password(jwt)
                        .set_keep_alive(super::mqtt::KeepAlive::Enabled { secs: 60 })
                        .set_user_name(&self.configuration.stcli.device_id)
                        .build();
                let mqtt_client = Arc::new(Mutex::new(mqtt_client));

                self.client = Some(mqtt_client.clone());
                self.connection = ConnectionState::WaitingConnection;

                let myself = ctx.myself.clone();
                let device_id = self.configuration.stcli.device_id.clone();

                // connect and subscribe
                tokio::spawn(async move {
                    let mut client = mqtt_client.lock().await;
                    let code = client.connect().await;

                    myself.send_msg(LifeCycleEvent::ConnectResult(code.clone()));
                    if code.is_err() {
                        return;
                    }

                    info!("connected");

                    let topics = [
                        (
                            format!("/v1/commands/{}", &device_id),
                            mqtt::QualityOfService::Level0,
                        ),
                        (
                            format!("/v1/notifications/{}", &device_id),
                            mqtt::QualityOfService::Level0,
                        ),
                    ];

                    let res = client.subscribe(topics.to_vec()).await;
                    myself.send_msg(LifeCycleEvent::SuscribeResult(res));

                    info!("subscribed");
                });
            }

            Cmd::Publish(event) => {
                if self.connection == ConnectionState::Connected {
                    let events = if self.pending_events.is_empty() {
                        DeviceEvents::one(event)
                    } else {
                        let mut events = std::mem::take(&mut self.pending_events);
                        self.pending_events = vec![];
                        events.push(event);
                        DeviceEvents::many(events)
                    };
                    let payload = serde_json::to_vec(&events).unwrap();

                    let topic = format!("/v1/deviceEvents/{}", self.configuration.stcli.device_id);
                    let msg = mqtt::Publish::new(
                        topic.clone(),
                        payload,
                        mqtt::QualityOfService::Level1, // TODO Make that configurable
                    );

                    if let Some(client) = &self.client {
                        let client = client.clone();
                        let myself = ctx.myself.clone();

                        tokio::spawn(async move {
                            let client = client.lock().await;
                            debug!("Sending event to '{}'", topic);
                            let res = client.publish(msg).await;

                            myself.send_msg(LifeCycleEvent::PublishResult(res));
                        });
                    }
                } else {
                    self.pending_events.push(event);
                }
            }
        }
    }
}

impl Receiver<LifeCycleEvent> for SmartThings {
    fn recv(&mut self, ctx: &Context<Self>, msg: LifeCycleEvent) {
        match msg {
            LifeCycleEvent::ConnectResult(result) => {
                match result {
                    Ok(()) => {
                        self.connection = ConnectionState::Connected;

                        // TODO register subsrciptions to this actor
                    }
                    Err(err) => {
                        // TODO verify if that's enough to drop the client
                        error!("Cannot connect to the cloud. Error = {:?}", err);
                        self.connection = ConnectionState::Disconnected;
                        self.client = None;
                    }
                }
            }
            LifeCycleEvent::PublishResult(Ok(())) => (),
            LifeCycleEvent::PublishResult(Err(error)) => {
                error!("Cannot publish some device events: {:?}", error);
            }
            LifeCycleEvent::SuscribeResult(Ok(_codes)) => {
                // TODO Check the codes and handle possible error
                use futures::StreamExt;
                if let Some(client) = &self.client {
                    let client = client.clone();

                    // This isn't good, and will be removed once the MQTT client will be actor
                    // based. In the meantime, we don't have that much concurrency going on and
                    // should be fine with a blocking operation here.
                    let subs = futures::executor::block_on(async {
                        let client = client.lock().await;

                        client.subscriptions().map(|result| {
                            result.map_err(|err| {
                                match err {
                                    tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(laged) => laged,
                                }
                            })
                        })
                    });

                    let handle = ctx.subscribe_to_stream(subs);
                    self.subscriptions_handle = Some(handle);
                }
            }
            LifeCycleEvent::SuscribeResult(Err(error)) => {
                error!("Couldn't subscribe to ST topics: {:?}", error)
            }
        }
    }
}

impl Message for Result<mqtt::SubMessage, u64> {}

impl Receiver<Result<mqtt::SubMessage, u64>> for SmartThings {
    fn recv(&mut self, _ctx: &Context<Self>, result: Result<mqtt::SubMessage, u64>) {
        match result {
            Ok(mqtt::SubMessage {
                topic_name,
                payload,
                ..
            }) => {
                // Not sure why we got trailing bytes, but let's remove them as a band aid
                let p = trim(&payload);

                match Topic::from_topic_name(&topic_name) {
                    Topic::Commands => {
                        let r = serde_json::from_slice::<Commands>(&p);
                        debug!("Received command: {:?}", r);
                        // TODO Handle error
                        if let Some(chan) = &self.commands {
                            for command in r.unwrap().commands {
                                chan.publish(command, "smartthings/command");
                            }
                        }
                    }
                    Topic::Notifications => {
                        let msg = String::from_utf8(p.to_vec());

                        debug!("Received notification: {:?}", msg);
                    }
                }
            }
            Err(lag) => {
                warn!("Commands consumer lag behind producer by {}", lag)
            }
        }
    }
}

// Remove leading and trailing 0 byte
fn trim(bytes: &[u8]) -> &[u8] {
    fn is_zero(c: &u8) -> bool {
        *c == 0
    }

    fn is_not_zero(c: &u8) -> bool {
        !is_zero(c)
    }

    if let Some(first) = bytes.iter().position(is_not_zero) {
        if let Some(last) = bytes.iter().rposition(is_not_zero) {
            &bytes[first..last + 1]
        } else {
            unreachable!();
        }
    } else {
        &[]
    }
}

/*

// We need to keep the broadcast::sender to be able to create new receiver at will
pub struct STask {
    device_id: String,
    client: Arc<client::MqttClient>,
    command_channel: broadcast::Sender<Command>,
    notification_channel: broadcast::Sender<()>,
    subs_handle: JoinHandle<()>,
}

impl STask {
    pub async fn send_event(&self, event: DeviceEvent) {
        let payload = serde_json::to_vec(&DeviceEvents::single(event)).unwrap();
        let topic = format!("/v1/deviceEvents/{}", self.device_id);
        let msg = client::Publish::new(
            topic,
            payload,
            client::QualityOfService::Level1, // TODO Make that configurable
        );
        self.client.publish(msg).await.unwrap();
    }

    pub async fn event_sink(&self) -> impl futures::Sink<DeviceEvent> {
        let client = self.client.clone();
        let id = self.device_id.clone();

        futures::sink::unfold((), move |_, event: DeviceEvent| {
            let client = client.clone();
            let id = id.clone();
            async move {
                // TODO Handle errors somehow
                let payload = serde_json::to_vec(&DeviceEvents::single(event)).unwrap();
                let topic = format!("/v1/deviceEvents/{}", id);
                let msg = client::Publish::new(
                    topic.clone(),
                    payload,
                    client::QualityOfService::Level1, // TODO Make that configurable
                );

                debug!("Sending event to '{}'", topic);
                client.clone().publish(msg).await.unwrap();
                Ok::<_, futures::never::Never>(())
            }
        })
    }

    pub fn commands(
        &self,
    ) -> impl Stream<Item = Result<Command, tokio_stream::wrappers::errors::BroadcastStreamRecvError>>
    {
        tokio_stream::wrappers::BroadcastStream::new(self.command_channel.subscribe())
    }

    #[allow(unused)]
    pub fn notifications(
        &self,
    ) -> impl Stream<Item = Result<(), tokio_stream::wrappers::errors::BroadcastStreamRecvError>>
    {
        tokio_stream::wrappers::BroadcastStream::new(self.notification_channel.subscribe())
    }

    pub async fn join(self) -> Result<(), tokio::task::JoinError> {
        self.subs_handle.await?;
        Ok(())
    }
}

pub(super) async fn spawn(cfg: &Configuration) -> Result<STask, Box<dyn std::error::Error>> {
    if cfg.onboarding.identity_type != "ED25519" {
        panic!(
            "Only ED25519 keys are supported at the moment. {} passed",
            cfg.onboarding.identity_type
        ); // TODO return an error
    }

    debug!("Configuration: {:#?}\n", cfg);
    let jwt = jwt::generate(&cfg)?;

    //
    // Hack something for MQTT and then try to find a good abstraction for other to use
    //

    //.server_uri("ssl://mqtt-regional-useast1.api.smartthings.com:8883")
    let mut mqtt_client =
        client::MqttClient::builder("mqtt-regional-useast1.api.smartthings.com", 8883)
            .set_client_id("mqtt-console-rs")
            .set_password(jwt)
            .set_keep_alive(client::KeepAlive::Enabled { secs: 60 })
            .set_user_name(&cfg.stcli.device_id)
            .build();

    // connect
    let code = mqtt_client.connect().await;
    debug!("client.connect() returned {:?}", code);

    let mut messages = mqtt_client.subscriptions();

    info!("connected");

    // subscribe
    let topics = [
        (
            format!("/v1/commands/{}", &cfg.stcli.device_id),
            client::QualityOfService::Level0,
        ),
        (
            format!("/v1/notifications/{}", &cfg.stcli.device_id),
            client::QualityOfService::Level0,
        ),
    ];
    mqtt_client.subscribe(topics.to_vec()).await?;

    info!("subscribed");

    let (command_channel, _) = broadcast::channel(5);
    let (notification_channel, _) = broadcast::channel(5);
    let command_sender = command_channel.clone();

    let subs_handle = tokio::spawn(async move {
        while let Some(msg_opt) = messages.next().await {
            if let Ok(msg) = msg_opt {
                // Not sure why we got trailing bytes, but let's remove them as a band aid
                let p = trim(&msg.payload);

                match Topic::from_topic_name(&msg.topic_name) {
                    Topic::Commands => {
                        let r = serde_json::from_slice::<Commands>(&p);
                        debug!("Received command: {:?}", r);
                        // TODO Handle error
                        for command in r.unwrap().commands {
                            command_sender.send(command).unwrap();
                        }
                    }
                    Topic::Notifications => {
                        let msg = String::from_utf8(p.to_vec());

                        debug!("Received notification: {:?}", msg);
                    }
                }
            } else {
                // A "None" means we were disconnected.
            }
        }
    });

    Ok(STask {
        device_id: cfg.stcli.device_id.clone(),
        client: Arc::new(mqtt_client),
        command_channel,
        notification_channel,
        subs_handle,
    })
}

*/
