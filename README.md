# my-st-home

Provides some features for my smart home:
- light control for constant brightness
- eink screen for an at a glance view of everything

## Tasks

Where we are and where we needs to go

[x] MTTQ Connection to ST
[x] Luminosity sensor measurements
[x] Lifx local control
[x] Control loop between luminosity and bulbs brightness
[x] Screen setup
[ ] Display relevant information on screen
[ ] Control on/off/brightness target through ST (MQTT connection, or hub-connected for latency (if doable))
[ ] Simplify configuration (hard coded to cfg file, cfg file to web ui)
[ ] Documentation
   [ ] Setting up the hardware
   [ ] Prep the raspberry
   [ ] Code doc & maybe clean up if needed
[ ] Make the tsl_2591 module depending on embedded-hal only and publish to crates.io


# Cross compiling

_Note: This is for targeting the raspberry pi. Depending on your device, the target may be different._


We use [cross](https://github.com/rust-embedded/cross/pull/522) to simplify the setup.

1. rustup toolchain add stable-x86_64-unknown-linux-gnu --profile=minimal
2. cargo install cross
3. cross build --target=armv7-unknown-linux-gnueabihf

The binary will be found in `target/armv7-unknown-linux-gnueabihf/debug/mqtt-console`.

Note: When doing a release, pass the `--release` flag and look in the `release` directory instead of `debug`.

