# my-st-home

Provides some features for my smart home:
- light control for constant brightness
- eink screen for an at a glance view of everything

## Tasks

Where we are and where we needs to go

- [x] MTTQ Connection to ST
- [x] Luminosity sensor measurements
- [x] Lifx local control
- [x] Control loop between luminosity and bulbs brightness
- [x] Screen setup
- [ ] Display relevant information on screen
  - [ ] Can start with project name, current lux reading, lux target, and brightness level
  - [ ] Then I can think of better UI with other features (weather, calendar, …)
  - [ ] Might need some basic layouting for the latter. Not sure how hardcoding every dimension will work.
- [x] Control on/off/brightness target through ST (MQTT connection, latency is around a second)
- [ ] Simplify configuration 
  - [ ] hard coded to cfg file
  - [ ] cfg file to web ui)
- [ ] Documentation
  - [ ] Setting up the hardware
  - [ ] Prep the raspberry
  - [ ] Code doc & maybe clean up if needed
- [ ] Make the tsl_2591 module depending on embedded-hal only and publish to crates.io
- [ ] Think on a method to configure the control loop by the end user
  - [ ] Might wait until I get my 3D printer, we'll see
  - [ ] Do some reading on PID (auto-) configuration
- [ ] Maybe temperature and PG&E integration ?


# Cross compiling

_Note: This is for targeting the raspberry pi. Depending on your device, the target may be different._


We use [cross](https://github.com/rust-embedded/cross/pull/522) to simplify the setup.

1. rustup toolchain add stable-x86_64-unknown-linux-gnu --profile=minimal
2. cargo install cross
3. cross build --target=armv7-unknown-linux-gnueabihf

The binary will be found in `target/armv7-unknown-linux-gnueabihf/debug/mqtt-console`.

Note: When doing a release, pass the `--release` flag and look in the `release` directory instead of `debug`.

