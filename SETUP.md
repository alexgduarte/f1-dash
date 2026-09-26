# Setup

A short tutorial on how to setup f1-dash via docker compose or kubernetes.

## Components

### dashboard

Techstack: Next.js, TypeSecript

The website/dashboard itself. Gets data from api and realtime service. Both URLs are used by the browser, so they have to be reachable from it.

envs:
```
NEXT_PUBLIC_LIVE_URL=http://localhost:4000
NEXT_PUBLIC_API_URL=http://localhost:4001    # API_URL is accepted too, for older setups

# rybbit tracking script and id
TRACKING_ID=
TRACKING_URL=
```

build envs:
```
SKIP_ENV_VALIDATION=1		# skips env validation, use for docker
NEXT_STANDALONE=1 			# enables nextjs standalone build, use for docker
NEXT_NO_COMPRESS=1 			# disables nextjs compression, use when using proxy compression
NEXT_EXPORT=1               # fully static build into ./out (what the native app bundles)
```

A static build (`yarn build:export`) can be served by any static file server. Its `NEXT_PUBLIC_*` values are fixed at build time.

note: you can't change build variables when using the public f1-dash image. But you can set them when building your own.

### realtime

Techstack: Rust, SignalR, Axum

Connects to the timing feeds and serves realtime data to the dashboard over server-sent events (`/api/realtime?series=f1`). Supported series: `f1`, `f2`, `f3`, `f1a` (F1 Academy) and `wec`.

envs:
```
# logging
RUST_LOG=realtime=info,feeds=info

# (optional) championships to ingest, comma separated, default all
SERIES=f1,f2,f3,f1a,wec

# Address where the webserver opens on with port
ADDRESS=0.0.0.0:4000

# CORS Origin, set to dashboard address
ORIGIN="https://f1-dash.com"

# (optional) endpoint for simulator
F1_DEV_URL=ws://localhost:8000/ws
```

### api

Techstack: Rust, Axum

Handles all non realtime data depended things like past & future sessions (`/api/schedule?series=f1`).

envs:
```
# logging
RUST_LOG=api=info

# Address where the webserver opens on with port
ADDRESS=0.0.0.0:4001

# CORS Origin, set to dashboard address
ORIGIN="https://f1-dash.com"
```

### A note on the feeds

The timing feeds are not official public APIs and their terms generally allow personal use. If you host f1-dash for other people, check the terms of the series you enable with `SERIES`.

## Platforms

Please not when choosing the dockerimages / choosing which tag, if you use latest or develop, which are moving tags and are not fixed, things might break over time.

### Docker Compose

There is a basic docker compose file in the root of the project. It builds the images from this repository. This is a very basic setup only expected to run on a local machine and only expected to be accessed from there.
If you want to host f1-dash on a server or also access it from other devices on your local network then adjustments have to be made like setting the ORIGIN environment variable or setting up a reverse proxy.

### Kubernetes

There is a kubernetes setup in the `.k8s` folder. This is a more advanced setup expected to run on a server and be accessed from other devices on your local network or even the internet.
This is the way f1-dash.com is hosted. There's no Helm Chart for now just a few yamls. Ingress/Gatway is not included as this is very specific to your setup.
