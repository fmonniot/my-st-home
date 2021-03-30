mod configuration;
mod delay;
mod lifx;
mod mqtt;
mod screen;

pub(crate) use configuration::Configuration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // TODO Change the .with_file_name to not have to pass a dummy file name here
    #[cfg(target_os = "linux")]
    let cfg = Configuration::from_directory("/home/pi/.mysthome/nothing")?;

    #[cfg(not(target_os = "linux"))]
    let cfg = Configuration::from_directory(
        "/Users/francoismonniot/Projects/local/my-st-home/data/project/nothing",
    )?;

    // Create our background processors (lifx, screen, mqtt, ST events)
    let _stdk = if false {
        mqtt::spawn(&cfg).await?
    } else {
    };
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
