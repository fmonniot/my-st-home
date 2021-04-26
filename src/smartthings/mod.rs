//! This module handle the connection between this project and
//! the SmartThings platform.
//!
//! At the moment, that means the MQTT connection to the cloud but we can
//! imagine that in the future we may be able to connect to the Hub directly
//! and have a fully local execution loop.

mod actor;
mod jwt;
mod mqtt;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

pub use actor::{new, Cmd, SmartThings};

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

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct DeviceEvents {
    device_events: Vec<DeviceEvent>,
}

impl DeviceEvents {
    fn one(event: DeviceEvent) -> DeviceEvents {
        DeviceEvents {
            device_events: vec![event],
        }
    }

    fn many<I: Into<Vec<DeviceEvent>>>(iter: I) -> DeviceEvents {
        DeviceEvents {
            device_events: iter.into(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
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

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EventVisibility {
    displayed: bool,
    non_archivable: bool,
    ephemeral: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct EventProviderData {
    timestamp: Option<u64>,
    sequence_number: Option<u64>,
    event_id: Option<String>,
    state_change: Option<EventStateChange>,
    raw_data: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum EventStateChange {
    Y,
    N,
    Unknown,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
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
