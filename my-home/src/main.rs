use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;

use my_home::configuration::Configuration;
use my_home::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    // TODO Should be part of the configuration, or at least the interface
    // should be. We can determine the broadcast address from it. Maybe
    // with default iface set to wifi ?
    let broadcast_addr: SocketAddr = "192.168.1.255:56700"
        .parse()
        .expect("correct hardcoded broadcast address");

    // TODO Change the .with_file_name to not have to pass a dummy file name here
    #[cfg(target_os = "linux")]
    let cfg = Configuration::from_directory("/home/pi/.mysthome/nothing")?;

    #[cfg(not(target_os = "linux"))]
    let cfg = Configuration::from_directory(
        "/Users/francoismonniot/Projects/local/my-st-home/data/project/nothing",
    )?;

    let system = actor::ActorSystem::new();

    let smartthings = smartthings::new(cfg.clone())?;
    let smartthings = system.actor_of("smartthings", smartthings).unwrap();

    smartthings.send_msg(smartthings::Cmd::Connect);

    let _sensors_actor = system
        .default_actor_of::<sensors::actors::Sensors>("sensors")
        .unwrap();

    let screen = system.actor_of("screen", screen::new()).unwrap();

    let lifx_actor = system
        .actor_of("lifx", lifx::actors::Manager::new(broadcast_addr))
        .unwrap();

    let adaptive_brightness = system
        .actor_of("logic/adaptive", logic::brightness::new(lifx_actor.clone()))
        .unwrap();

    let _st_state = system
        .actor_of(
            "logic/st_state",
            logic::st_state::new(adaptive_brightness, lifx_actor, smartthings),
        )
        .unwrap();

    // Create our background processors (lifx, screen, mqtt, ST events)
    let s_task = mqtt::spawn(&cfg).await?;
    let screen_task = screen::spawn();
    let lifx = lifx::spawn().await?; // TODO Use actor instead
    let sensors = sensors::spawn();

    let screen_handle = screen_task.handle();

    // Dummy thing, to avoid unused warn until we have the real logic
    screen_handle.update(screen::ScreenMessage::UpdateLifxBulb {
        source: 0,
        power: true,
    })?;
    screen.send_msg(screen::ScreenMessage::UpdateLifxBulb {
        source: 0,
        power: true,
    });

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
    // Will be required once we are fully in actors, because we don't have (yet)
    // a way to wait for all actors to be stopped.

    // At the end, await the end of the background processes
    sensors.join().await?;
    lifx.join_handle().await?;
    screen_task.join().await?;
    s_task.join().await?;

    Ok(())
}
