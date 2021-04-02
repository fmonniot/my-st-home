use ed25519_dalek::{Keypair, PublicKey, SecretKey, Signer};
use futures::stream::StreamExt;
use log::{debug, info};
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

use super::Configuration;

mod client;

pub(super) async fn spawn(cfg: &Configuration) -> Result<(), Box<dyn std::error::Error>> {
    if cfg.onboarding.identity_type != "ED25519" {
        println!(
            "Only ED25519 keys are supported at the moment. {} passed",
            cfg.onboarding.identity_type
        );
        return Ok(()); // TODO return an error
    }

    debug!("Configuration: {:#?}\n", cfg);

    // Key deserialization experimentation (could probably go into configuration)
    let public_key_bytes = base64::decode(&cfg.device_info.public_key)?;
    let private_key_bytes = base64::decode(&cfg.device_info.private_key)?;

    let public_key: PublicKey = PublicKey::from_bytes(&public_key_bytes)?;
    let secret_key: SecretKey = SecretKey::from_bytes(&private_key_bytes)?;
    let keypair = Keypair {
        public: public_key,
        secret: secret_key,
    };

    let header = Header::with_serial(&cfg.device_info.serial_number);
    let body = Body::generate(cfg.onboarding.mn_id.clone());
    let jwt = generate_jwt(header, body, &keypair)?;

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

    tokio::spawn(async move {
        while let Some(msg_opt) = messages.next().await {
            if let Ok(msg) = msg_opt {
                // Not sure why we got trailing bytes, but let's remove them as a band aid
                let p = trim(&msg.payload);

                match Topic::from_topic_name(&msg.topic_name) {
                    Topic::Commands => {
                        let r = serde_json::from_slice::<Commands>(&p);
                        debug!("Received command: {:?}", r);
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

    let topic = format!("/v1/deviceEvents/{}", cfg.stcli.device_id);
    let msg = client::Publish::new(
        topic,
        "{}".as_bytes().to_vec(),
        client::QualityOfService::Level1,
    );
    mqtt_client.publish(msg).await?;

    //mqtt_client.disconnect().await?;

    Ok(())
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

// JWT (custom implementation for now. TODO see if we can use a standard crate)

fn generate_jwt(
    header: Header,
    body: Body,
    keypair: &Keypair,
) -> Result<String, Box<dyn std::error::Error>> {
    let h = serde_json::to_vec(&header)?;
    let b = serde_json::to_vec(&body)?;

    let h2 = base64::encode(h);
    let b2 = base64::encode(b);

    let msg = format!("{}.{}", h2, b2);
    let signature = keypair.try_sign(msg.as_bytes())?;

    Ok(format!("{}.{}.{}", h2, b2, base64::encode(signature)))
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Header {
    alg: String,
    kty: String,
    crv: String,
    typ: String,
    ver: String,
    kid: String,
}

impl Header {
    fn with_serial(serial: &str) -> Header {
        Header {
            alg: "EdDSA".to_string(),
            kty: "OKP".to_string(),
            crv: "ed25519".to_string(),
            typ: "JWT".to_string(),
            ver: "0.0.1".to_string(),
            kid: serial.to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Body {
    iat: String,
    jti: String,
    mn_id: String,
}

impl Body {
    fn generate(mn_id: String) -> Body {
        let jti = uuid::Uuid::new_v4().to_hyphenated().to_string();
        let sys_time = SystemTime::now();
        let iat = sys_time
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let iat = format!("{}", iat);

        Body { iat, jti, mn_id }
    }
}

// ST models
use uuid::Uuid;
use serde_json::Value;


#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct DeviceEvents {
    // deviceEvents
    device_events: Vec<DeviceEvent>
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct DeviceEvent {
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

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct EventVisibility {
    displayed: bool,
    non_archivable: bool,
    ephemeral: bool,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct EventProviderData {
    timestamp: Option<u64>,
    sequence_number: Option<u64>,
    event_id: Option<String>,
    state_change: Option<EventStateChange>,
    raw_data: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
enum EventStateChange {
    Y,
    N,
    Unknown
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Commands {
    commands: Vec<Command>
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Command {
    id: Uuid,
    component: String,
    capability: String,
    command: String,
    arguments: Vec<Value>
}
