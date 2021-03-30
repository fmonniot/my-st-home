use serde::{Deserialize, Serialize};
use std::{fs::File, io::BufReader, path::Path};

mod delay;
mod lifx;
mod mqtt;
mod screen;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // TODO Change the .with_file_name to not have to pass a dummy file name here
    #[cfg(target_os = "linux")]
    let cfg = Configuration::from_directory("/home/pi/.mysthome/nothing")?;

    #[cfg(not(target_os = "linux"))]
    let cfg = Configuration::from_directory(
        "/Users/francoismonniot/Projects/local/my-st-home/data/project/nothing",
    )?;

    // Create our background processors (lifx, screen, mqtt, ST events)
    let stdk = mqtt::spawn(&cfg).await?;
    let (screen_join_handle, screen_handle) = screen::spawn();
    let lifx = lifx::spawn().await?;

    // Then create the handle to use in the business loop
    let _lifx_handle = lifx.handle();

    // Dummy thing, to avoid unused warn until we have the real logic
    screen_handle.update(screen::ScreenMessage::UpdateLifxBulb {
        source: 0,
        power: true,
    })?;

    // At the end, await the end of the background processes
    screen_join_handle.await?;
    lifx.join_handle().await?;
    //mqtt::spawn(&cfg).await?;

    Ok(())
}

// Configuration

#[derive(Debug)]
struct Configuration {
    device_info: DeviceInfo,
    stcli: StCli,
    meta: MetaFile,
    onboarding: OnboardingConfig,
}

impl Configuration {
    fn from_directory<P: AsRef<Path>>(p: P) -> Result<Configuration, Box<dyn std::error::Error>> {
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
struct DeviceInfoFile {
    device_info: DeviceInfo,
    stcli: StCli,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct DeviceInfo {
    firmware_version: String,
    private_key: String,
    public_key: String,
    serial_number: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct StCli {
    device_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct MetaFile {
    location_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct OnboardingConfigFile {
    onboarding_config: OnboardingConfig,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct OnboardingConfig {
    device_onboarding_id: String,
    mn_id: String,
    setup_id: String,
    vid: String,
    device_type_id: String,
    identity_type: String,
    device_integration_profile_key: DipKey,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct DipKey {
    id: String,
    major_version: u8,
    minor_version: u8,
}
