# httpbounder
Broadcasts HTTP streams, synchronizes with MJPEG boundary.


Also works with any `Transfer-Encoding: chunked`.

```
USAGE:
    httpbounder [OPTIONS] --input <input>

OPTIONS:
    -b, --bind <bind>        actix HttpServer bind addr [default: 0.0.0.0:8080]
    -i, --input <input>      http stream URL, eg. http://1.2.3.4/mjpg/video.mjpg
    -o, --output <output>    http stream path, eg. /video.mjpg [default: /video.mjpg]
    -u, --user <user>        example: 'user:password'

```

## Example
```
./httpbounder -i "http://192.168.0.3/mjpg/video.mjpg" -u "admin:pass" -o "/broadcast.mjpg"
```
```
curl http://127.0.0.1:8080/broadcast.mjpg
```

## How it works
Typical stream:
```
chunk: b"\x69\x69\x69\x69\x69\x69\r\n--myboundary\r\n"
chunk: b"Motion-Event: 0\r\n"
chunk: b"Content-Type: image/jpeg\r\nContent-Length: 189712\r\n\r\n\xff\xdd\x69\x69\x69"
```

Client has to receive: `--myboundary\r\n` bytes first.
Chunks of bytes are split in order to broadcast valid streams.

Resulting in:
```
chunk: b"--myboundary\r\n"
chunk: b"Motion-Event: 0\r\n"
chunk: b"Content-Type: image/jpeg\r\nContent-Length: 189712\r\n\r\n\xff\xdd\x69\x69\x69"
```

## Multiple streams
`Streams.toml`:

```
[[streams]]
user = "admin:pass"
url = "http://192.168.0.3/mjpg/video.mjpg"
output_path = "/cam1"
```

```
./httpbounder -i "Streams.toml"
```