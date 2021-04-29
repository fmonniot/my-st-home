//! JWT (custom implementation for now. TODO see if we can use a standard crate)
use ed25519_dalek::{Keypair, PublicKey, SecretKey, Signer};
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

use crate::configuration::Configuration;

pub(crate) fn generate(cfg: &Configuration) -> Result<String, Box<dyn std::error::Error>> {
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

    Ok(jwt)
}

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
