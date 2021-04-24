mod actor;
mod configuration;
mod delay;
mod lifx;
mod logic;
mod mqtt;
mod screen;
mod sensors;

#[cfg(target_os = "linux")]
mod tsl_2591;

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;

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

    let system = actor::ActorSystem::new();
    let _sensors_actor = system
        .default_actor_of::<sensors::actors::Sensors>("sensors")
        .unwrap();

    let broadcast_addr: SocketAddr = "192.168.1.255:56700"
        .parse()
        .expect("correct hardcoded broadcast address");

    let lifx_actor = system
        .actor_of("lifx", lifx::actors::Manager::new(broadcast_addr))
        .unwrap();

    let _adaptive_brightness = system
        .actor_of("logic/adaptive", logic::brightness::new(lifx_actor))
        .unwrap();

    // Create our background processors (lifx, screen, mqtt, ST events)
    let s_task = mqtt::spawn(&cfg).await?;
    let (screen_task, window_run_loop) = screen::spawn();
    let lifx = lifx::spawn().await?; // TODO Use actor instead
    let sensors = sensors::spawn();

    let screen_handle = screen_task.handle();
    // Dummy thing, to avoid unused warn until we have the real logic
    screen_handle.update(screen::ScreenMessage::UpdateLifxBulb {
        source: 0,
        power: true,
    })?;

    // No state persistence for now
    let light_state = Arc::new(RwLock::new(false));
    // Notify the cloud in which state we are
    s_task
        .send_event(mqtt::DeviceEvent::simple_str(
            "main", "switch", "switch", "off",
        ))
        .await;
    // Turn off the lifx bulbs too ?

    tokio::spawn(logic::adaptive_brightness(
        sensors.messages(),
        lifx.handle(),
        light_state.clone(),
        screen_task.handle(),
    ));

    let event_sink = s_task.event_sink().await;
    tokio::spawn(logic::st_light_state(
        s_task.commands(),
        event_sink,
        lifx.handle(),
        light_state.clone(),
        screen_task.handle(),
    ));

    // TODO Need something to trigger a stop of the various background processes
    // rust signal handling ?

    // Install the window run loop (require main thread).
    // Maybe we should find another way to add what is effectively debug code
    // (Maybe a different tool to test out the different frames ?)
    // Note that on mac, this consume the signal received and as such we need
    // two ctrl-c to exit.
    window_run_loop();

    // At the end, await the end of the background processes
    sensors.join().await?;
    lifx.join_handle().await?;
    screen_task.join().await?;
    s_task.join().await?;

    Ok(())
}
