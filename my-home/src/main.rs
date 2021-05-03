use std::net::SocketAddr;

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
        "/Users/francoismonniot/Projects/github.com/fmonniot/my-st-home/data/project/nothing",
    )?;

    let system = actor::ActorSystem::new();

    let smartthings = smartthings::new(cfg.clone())?;
    let smartthings = system.actor_of("smartthings", smartthings).unwrap();

    smartthings.send_msg(smartthings::Cmd::Connect);

    let _sensors_actor = system
        .default_actor_of::<sensors::Sensors>("sensors")
        .unwrap();

    let _screen = system.actor_of("screen", screen::new()).unwrap();

    let lifx_actor = system
        .actor_of("lifx", lifx::Manager::new(broadcast_addr))
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

    // TODO Need something to trigger a stop of the various background processes
    // rust signal handling ?
    // Will be required once we are fully in actors, because we don't have (yet)
    // a way to wait for all actors to be stopped.

    // Only used to not exit the main function. Could be hidden inside the actor system and wait
    // for all actors to have terminated.
    // The System could listen to ctrl-c and terminate actors on reception.
    let (_, rx) = tokio::sync::oneshot::channel();
    let _: () = rx.await?;

    Ok(())
}
