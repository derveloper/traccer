# traccer

hook into processes via `LD_PRELOAD` to trace (http) request and send traces to jaeger

## WARNING: highly experimental!

This library is for educational purposes, it doesn't do what it pretend to, it evenually will do it, do not use it for production!

## Usage

you can use traccer via `LD_PRELOAD`:

```shell
cargo build && LD_PRELOAD=target/debug/libmylib.so curl google.de
```