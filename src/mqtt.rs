use ed25519_dalek::{Keypair, PublicKey, SecretKey, Signer};
use futures::stream::StreamExt;
use paho_mqtt as mqtt;
use serde::{Deserialize, Serialize};
use std::time::SystemTime;
use std::time::Duration;
use tokio::time::sleep;

use super::Configuration;


pub(super) async fn spawn(cfg: &Configuration) -> Result<(), Box<dyn std::error::Error>> {
    if cfg.onboarding.identity_type != "ED25519" {
        println!(
            "Only ED25519 keys are supported at the moment. {} passed",
            cfg.onboarding.identity_type
        );
        return Ok(()); // TODO return an error
    }

    println!("Configuration: {:#?}\n", cfg);

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

    let mut mqtt_client = mqtt::CreateOptionsBuilder::new()
        .server_uri("ssl://mqtt-regional-useast1.api.smartthings.com:8883")
        .client_id("mqtt-console-rs")
        .persistence(None)
        .create_client()?;

    // Need to pass those, otherwise the c lib fails on NULL…
    let ssl_opts = mqtt::SslOptionsBuilder::new().finalize();

    let conn_options = mqtt::ConnectOptionsBuilder::new()
        .clean_session(true)
        .user_name(&cfg.stcli.device_id)
        .password(jwt)
        .mqtt_version(mqtt::MQTT_VERSION_3_1_1)
        .ssl_options(ssl_opts)
        .finalize();

    // connect
    mqtt_client.connect(conn_options).await?;
    let mut messages = mqtt_client.get_stream(25);

    println!("connected");

    // subscribe
    let topics = [
        format!("/v1/commands/{}", &cfg.stcli.device_id),
        format!("/v1/notifications/{}", &cfg.stcli.device_id),
    ];
    let topics_qos = [1, 1];
    mqtt_client.subscribe_many(&topics, &topics_qos).await?;

    println!("subscribed");

    tokio::spawn(async move {
        while let Some(msg_opt) = messages.next().await {
            if let Some(msg) = msg_opt {
                let topic = msg.topic();
                let _payload = msg.payload();

                println!("Received message on {}: {}", topic, msg.payload_str());
            } else {
                // A "None" means we were disconnected.
            }
        }
    });

    //let topic = format!("/v1/deviceEvents/{}", cfg.stcli.device_id);
    //let msg = mqtt::Message::new(topic, "{}", mqtt::QOS_1);
    //mqtt_client.publish(msg).await?;

    sleep(Duration::from_millis(30_000)).await;
    println!("Sleep expired, closing");

    mqtt_client.disconnect(None).await?;

    Ok(())
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