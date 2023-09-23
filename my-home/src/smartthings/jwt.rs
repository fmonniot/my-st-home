//! JWT (custom implementation for now. TODO see if we can use a standard crate)

use base64ct::{Base64, Encoding};
use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

use crate::configuration::Configuration;

pub(crate) fn generate(cfg: &Configuration) -> Result<String, JwtError> {
    // Key deserialization experimentation (could probably go into configuration)
    let mut key_bytes = [0u8; 64];
    let signing_key = Base64::decode_vec(&cfg.device_info.private_key)?;
    let verifying_key = Base64::decode_vec(&cfg.device_info.public_key)?;

    if signing_key.len() != 32 {
        return Err(JwtError::InvalidKeyLength("private", signing_key.len()));
    }
    if verifying_key.len() != 32 {
        return Err(JwtError::InvalidKeyLength("public", verifying_key.len()));
    }

    key_bytes[..32].copy_from_slice(&signing_key);
    key_bytes[32..].copy_from_slice(&verifying_key);

    let keypair = SigningKey::from_keypair_bytes(&key_bytes)?;

    let header = Header::with_serial(&cfg.device_info.serial_number);
    let body = Body::generate(cfg.onboarding.mn_id.clone());
    let jwt = generate_jwt(header, body, &keypair)?;

    Ok(jwt)
}

fn generate_jwt(header: Header, body: Body, keypair: &SigningKey) -> Result<String, JwtError> {
    let h = serde_json::to_vec(&header)?;
    let b = serde_json::to_vec(&body)?;

    let h2 = Base64::encode_string(&h);
    let b2 = Base64::encode_string(&b);

    let msg = format!("{}.{}", h2, b2);
    let signature = keypair.try_sign(msg.as_bytes())?;

    Ok(format!(
        "{}.{}.{}",
        h2,
        b2,
        Base64::encode_string(&signature.to_bytes())
    ))
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
        let jti = uuid::Uuid::new_v4().hyphenated().to_string();
        let sys_time = SystemTime::now();
        let iat = sys_time
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let iat = format!("{}", iat);

        Body { iat, jti, mn_id }
    }
}


#[derive(thiserror::Error, Debug)]
pub enum JwtError {
    #[error("The given {0} key should have 32 bytes but had {1}")]
    InvalidKeyLength(&'static str, usize),

    #[error("Couldn't decode key from base64 encoded string")]
    DecodeBase64(#[from] base64ct::Error),

    #[error("Invalid ED25519 signature")]
    Signature(#[from] ed25519_dalek::SignatureError),

    #[error("Couldn't serialize header or body")]
    Serialization(#[from] serde_json::Error),
}
