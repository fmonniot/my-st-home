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
