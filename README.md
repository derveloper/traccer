# traccer

hook into processes via `LD_PRELOAD` to trace (http) request and send traces to jaeger

## WARNING: highly experimental!

This library is for educational purposes, it doesn't do what it pretend to, it evenually will do it, do not use it for production!

## Usage

start jaeger:
```shell
docker run -d --name jaeger \
  -e COLLECTOR_ZIPKIN_HOST_PORT=:9411 \
  -p 5775:5775/udp \
  -p 6831:6831/udp \
  -p 6832:6832/udp \
  -p 5778:5778 \
  -p 16686:16686 \
  -p 14268:14268 \
  -p 14250:14250 \
  -p 9411:9411 \
  jaegertracing/all-in-one:1.22
```

then you can use traccer via `LD_PRELOAD`:

```shell
cargo build && LD_PRELOAD=target/debug/libtraccer.so perl -MLWP::UserAgent -le 'print LWP::UserAgent->new(requests_redirectable => [])->get(shift)->decoded_content()' "http://httpbin.org/delay/2"
```

and see your tasty traces [http://localhost:16686/](http://localhost:16686/)