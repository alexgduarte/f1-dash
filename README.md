<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="./dashboard/public/tag-logo.png" width="200">
    <img alt="f1-dash" src="./dashboard/public/tag-logo.png" width="200">
  </picture>
</p>

<h1 align="center">Real-time motorsport telemetry and timing</h1>

## f1-dash

A real-time timing dashboard for Formula 1, FIA Formula 2, FIA Formula 3, F1 Academy and the FIA World Endurance Championship. It shows the leaderboard, tyres, gaps, laps, sectors, race control, team radio, the tyre sets every driver has left for the race, and a rain radar around the circuit.

It runs in the browser and as a native app on Windows, macOS (Apple Silicon and Intel), Linux, iPhone, iPad and Android. The apps connect to the timing feeds straight from the device, so they need no f1-dash server.

## Features

| | F1 | F2 / F3 / F1 Academy | WEC |
|---|---|---|---|
| Leaderboard, gaps, lap and sector times | ✓ | ✓ | ✓ (with class positions and crews) |
| Mini sectors and track map | ✓ | – | – |
| Current tyre and stint | ✓ | – | ✓ (compound and age) |
| Tyre sets available for the race | ✓ | – | – |
| Race control messages | ✓ | ✓ (commentary feed) | ✓ (race log) |
| Team radio | ✓ | – | – |
| Weather readings and rain radar | ✓ | ✓ | ✓ (when the feed has weather) |
| Session clock, track status | ✓ | ✓ | ✓ (including FCY and Code 60) |
| Schedule | ✓ | ✓ | ✓ |
| Session replay | ✓ | – | – |

A dash means the series' feed does not publish that data.

### Session replay

Any finished F1 session since 2018 can be watched again from the live timing archive: pick it on the Replay page, then play, pause, change the speed (up to 32×) or drag the timeline. Replays go through the same dashboard as live sessions, tyre sets and team radio included. Car telemetry and positions are not replayed; they are large and often missing from the archive.

### Tyre sets for the race

For every F1 driver the dashboard lists the sets still available, each marked new or used with its lap count, and the set on the car now. It rebuilds this from every session of the weekend:

1. It starts from the allocation in the FIA Sporting Regulations: 13 dry sets (2 hard, 3 medium, 8 soft) at a standard event, 12 (2 hard, 4 medium, 6 soft) at a sprint event, plus 5 intermediate and 2 wet sets.
2. It reads the stints of the earlier sessions from the F1 live timing archive and the current session from the live feed. Tyre age carries over between sessions, so a used set's starting age identifies which earlier set it is.
3. It takes out the sets handed back to Pirelli after each session: 2 after each practice session and 1 soft for cars that reached Q3 at a standard event; 1 after FP1, the most used set of the sprint, and 3 after qualifying at a sprint event. At a standard event the regulations protect one set of each race compound and one soft for Q3, and those are never taken out early.

Teams choose most of the returned sets and no feed says which, so those are estimated: the most worn sets are assumed to go back first, and a set that appears again in a later session is known to have been kept. The sprint's most used set is prescribed by the rules, not estimated. Pirelli publishes the official list only as an image, so the dashboard labels the result as an estimate. The rules are in [`feeds/src/tyres/rules.rs`](feeds/src/tyres/rules.rs).

### Weather radar

The dashboard and the weather page show [RainViewer](https://www.rainviewer.com/) radar around the circuit with a timeline, the live track and air temperature, humidity, rain and a wind direction arrow.

## How it's built

```
dashboard/          Next.js front end, shared by the website and the apps
dashboard/src-tauri Tauri app (desktop and mobile) that runs the feeds in-process
feeds/              Feed adapters for every series, the tyre set tracker, replays, schedules
signalr/            SignalR clients (ASP.NET Core for F1, classic for F2/F3/F1 Academy)
realtime/           Server that runs the feeds for the website (server-sent events)
api/                Server for schedules
simulator/          Records and replays F1 sessions for development
```

Every adapter translates its series into the F1 live timing layout, so one set of components renders them all. Data sources:

- **F1**: the official live timing feed and its static archive of past sessions.
- **F2, F3, F1 Academy**: the shared FIA Formula 2/3 timing service.
- **WEC**: GriiipLive, the platform behind the official WEC timing page.
- **Schedules**: the official F1 calendar, the community [sportstimes](https://github.com/sportstimes/f1) calendars for F2, F3 and F1 Academy, and GriiipLive for WEC.

None of these feeds are official public APIs. They can change without notice, and their terms generally allow personal use. The apps connect from your own device; if you host the website for others, check the terms of the feeds you enable (`SERIES`, see [`SETUP.md`](SETUP.md)).

## Apps

Installers for Windows, macOS and Linux are attached to the [releases](https://github.com/alexgduarte/f1-dash/releases). Android and iOS builds come from the same workflow ([`.github/workflows/app.yaml`](.github/workflows/app.yaml)); installing them outside the test builds needs signing keys, described in [`CONTRIBUTING.md`](CONTRIBUTING.md#native-app).

## Contributing

Read [`CONTRIBUTING.md`](CONTRIBUTING.md) to set up f1-dash on your machine, and [`SETUP.md`](SETUP.md) to host it.

## Credits

f1-dash was created by [slowlydev](https://github.com/slowlydev/f1-dash), who now builds [Nitrous](https://nitrous.software). This fork adds the other championships, the native apps, tyre set tracking and the weather radar.

## Notice

This project/website is unofficial and is not associated in any way with the Formula 1 companies. F1, FORMULA ONE, FORMULA 1, FIA FORMULA ONE WORLD CHAMPIONSHIP, GRAND PRIX and related marks are trade marks of Formula One Licensing B.V. It is likewise not associated with the FIA, the FIA Formula 2 and Formula 3 Championships, F1 Academy, the FIA World Endurance Championship, Griiip or Pirelli.
