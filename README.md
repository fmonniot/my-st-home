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
- [x] Improve the the tsl_2591 module
  - [x] depends on embedded-hal only
  - [x] Integrate within `my-home` with `rppal`
  - [ ] publish to crates.io
- [ ] Think on a method to configure the control loop by the end user
  - [ ] Might wait until I get my 3D printer, we'll see
  - [ ] Do some reading on PID (auto-) configuration
- [ ] Maybe temperature and PG&E integration ?


# Cross compiling

_Note: This is for targeting the raspberry pi. Depending on your device, the target may be different._


We use [cross](https://github.com/rust-embedded/cross/pull/522) to simplify the setup.

1. rustup toolchain add stable-x86_64-unknown-linux-gnu --profile=minimal
2. cargo install cross
3. `cross build --target=armv7-unknown-linux-gnueabihf -p my-home`

The binary will be found in `target/armv7-unknown-linux-gnueabihf/debug/mqtt-console`.

Note: When doing a release, pass the `--release` flag and look in the `release` directory instead of `debug`.

Note 2: the `cross` docker image doesn't have SDL2, so we shouldn't try to build the `ui-designer` project in that environment. Selecting the correct crate with `-p` is thus important.

# Running on mac/windows

TODO Will be obsolete with the `ui-designer` sub-crate.

When not targeting `linux`, we are using [https://github.com/embedded-graphics/simulator] to render our screen.
This requires the SDL library to be installed on the system, lookup the link for instruction on how to do it.

# Repository layout

This repo contains multiple crates:

- `my-home` contains the code for what is being run on the device. It contains all the logic and important bits of this project.
- `tsl_2591` is the driver for the luminosity sensor we use in this project. It should eventually be published on crates.io once ported to use `embedded-hal` instead of `rppal`.
- `ui-designer` is a helper crate which provides a simulator for the UI rendering of the main project. The idea being that testing UI changes on the raspberry pi device is too slow, so we have a simulator being able to run on mac/linux and do the design work there.

Quick commands:
```
$ cargo test               # run tests on all crates
$ cargo run -p my-home     # run the main program
$ cargo run -p ui-designer # run the frame rendered
```
