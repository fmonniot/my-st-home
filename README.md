# mqtt-console

## MQTT logs

A number of environment variables control runtime tracing of the Paho C library.

Tracing is switched on using `MQTT_C_CLIENT_TRACE` (a value of `ON` traces to stdout, any other value should specify a file to trace to).

The verbosity of the output is controlled using the `MQTT_C_CLIENT_TRACE_LEVEL` environment variable - valid values are `ERROR`, `PROTOCOL`, `MINIMUM`, `MEDIUM` and `MAXIMUM` (from least to most verbose).

The variable `MQTT_C_CLIENT_TRACE_MAX_LINES` limits the number of lines of trace that are output.

```
export MQTT_C_CLIENT_TRACE=ON
export MQTT_C_CLIENT_TRACE_LEVEL=PROTOCOL
```


# Cross compiling

_Note: This is for compiling to the raspberry pi. Depending on your device, the target may be different._


We use [cross](https://github.com/rust-embedded/cross/pull/522) to simplify the setup.

1. rustup toolchain add stable-x86_64-unknown-linux-gnu --profile=minimal
2. cargo install cross
3. cross build --target=armv7-unknown-linux-gnueabihf

The binary will be found in `target/armv7-unknown-linux-gnueabihf/debug/mqtt-console`.

Note: When doing a release, pass the `--release` flag and look in the `release` directory instead of `debug`.

