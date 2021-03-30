use serde::{Deserialize, Serialize};
use std::{fs::File, io::BufReader, path::Path};

#[derive(Debug)]
pub struct Configuration {
    pub device_info: DeviceInfo,
    pub stcli: StCli,
    pub meta: MetaFile,
    pub onboarding: OnboardingConfig,
}

impl Configuration {
    pub fn from_directory<P: AsRef<Path>>(
        p: P,
    ) -> Result<Configuration, Box<dyn std::error::Error>> {
        let path = p.as_ref();

        let di: DeviceInfoFile = read_json_from_file(path.with_file_name("device_info.json"))?;
        let meta: MetaFile = read_json_from_file(path.with_file_name("meta.json"))?;
        let of: OnboardingConfigFile =
            read_json_from_file(path.with_file_name("onboarding_config.json"))?;

        Ok(Configuration {
            device_info: di.device_info,
            stcli: di.stcli,
            meta,
            onboarding: of.onboarding_config,
        })
    }

    // TODO Include JWT generation here and don't expose keys ?
    // TODO Don't expose field but use accessor instead ? Would let me refactor the config on disk easily.
}

fn read_json_from_file<P: AsRef<Path>, T: serde::de::DeserializeOwned>(
    path: P,
) -> Result<T, Box<dyn std::error::Error>> {
    // Open the file in read-only mode with buffer.
    println!("Reading file: {:?}", path.as_ref());
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let t = serde_json::from_reader(reader)?;

    Ok(t)
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfoFile {
    pub device_info: DeviceInfo,
    pub stcli: StCli,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub firmware_version: String,
    pub private_key: String,
    pub public_key: String,
    pub serial_number: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct StCli {
    pub device_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MetaFile {
    pub location_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct OnboardingConfigFile {
    onboarding_config: OnboardingConfig,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingConfig {
    pub device_onboarding_id: String,
    pub mn_id: String,
    setup_id: String,
    vid: String,
    device_type_id: String,
    pub identity_type: String,
    device_integration_profile_key: DipKey,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DipKey {
    id: String,
    major_version: u8,
    minor_version: u8,
}
