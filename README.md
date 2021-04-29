# traccer

hook into processes via `LD_PRELOAD` to trace (http) request and send traces to jaeger

## WARNING: highly experimental!

This library is for educational purposes, it doesn't do what it pretend to, it evenually will do it, do not use it for production!

## Usage

you can use traccer via `LD_PRELOAD`:

```shell
cargo build && LD_PRELOAD=target/debug/libtraccer.so perl -MLWP::UserAgent -le 'print LWP::UserAgent->new(requests_redirectable => [])->get(shift)->decoded_content()' "http://httpbin.org/delay/2"
```