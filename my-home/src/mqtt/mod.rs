use futures::stream::{Stream, StreamExt};
use log::{debug, info};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::{sync::broadcast, task::JoinHandle};
use uuid::Uuid;

use super::Configuration;

mod client;
mod jwt;

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

pub async fn spawn(cfg: &Configuration) -> Result<STask, Box<dyn std::error::Error>> {
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

enum Topic {
    Notifications,
    Commands,
}

impl Topic {
    fn from_topic_name(name: &str) -> Topic {
        if name.starts_with("/v1/commands") {
            Topic::Commands
        } else if name.starts_with("/v1/notifications") {
            Topic::Notifications
        } else {
            panic!("Unknown topic {}", name)
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

// ST models

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct DeviceEvents {
    device_events: Vec<DeviceEvent>,
}

impl DeviceEvents {
    fn single(event: DeviceEvent) -> DeviceEvents {
        DeviceEvents {
            device_events: vec![event],
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DeviceEvent {
    component: String,
    capability: String,
    attribute: String,
    value: Value,
    unit: Option<String>,
    data: Option<Value>,
    command_id: Option<Uuid>,
    visibility: Option<EventVisibility>,
    provider_data: Option<EventProviderData>,
}

impl DeviceEvent {
    pub fn simple_str(
        component: &str,
        capability: &str,
        attribute: &str,
        value: &str,
    ) -> DeviceEvent {
        DeviceEvent {
            component: component.to_string(),
            capability: capability.to_string(),
            attribute: attribute.to_string(),
            value: json!(value),
            unit: None,
            data: None,
            command_id: None,
            visibility: None,
            provider_data: None,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EventVisibility {
    displayed: bool,
    non_archivable: bool,
    ephemeral: bool,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EventProviderData {
    timestamp: Option<u64>,
    sequence_number: Option<u64>,
    event_id: Option<String>,
    state_change: Option<EventStateChange>,
    raw_data: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub enum EventStateChange {
    Y,
    N,
    Unknown,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Commands {
    commands: Vec<Command>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Command {
    id: Uuid,
    component: String,
    capability: String,
    pub command: String,
    arguments: Vec<Value>,
}
