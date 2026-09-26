# How to contribute

## Setup

You will need to install the following tools:

- [nvm](https://github.com/nvm-sh/nvm) (or [fnm](https://fnm.vercel.app/), [nvm-windows](https://github.com/coreybutler/nvm-windows))
- rust & cargo ([rustup](https://rustup.rs) is highly recommended)

To get started with the frontend do the following:

> [!NOTE]
> You will need multiple terminal sessions, if you want to run everything,
> you will need 4 sessions. (frontend, live backend, api backend, simulator).
> Also the following commands assume Linux, macOS or WSL. Windows commands may differ.

```bash
# Clone the repository or your fork
git clone git@github.com:slowlydev/f1-dash.git

# Go to the frontend
cd dash/

# Install the correct node version using nvm, fnm or nvm-windows
nvm install

# Enable corepack
corepack enable

# Install the package manager (yarn) with corepack
corepack install

# Install frontend dependencies
yarn

# Copy the env example and maybe adjust envs if some ports are already in use
cp .env.example .env

# To start development
yarn dev
```

Before we can use the frontend and start developing it, we need to set up the backend.
From here on we enter the Rust part, so make sure to have it installed.

```bash
cd f1-dash/

# If you haven't installed rust & cargo run the following
rustup toolchain install

# Copy the env example and maybe adjust envs if some ports are already in use
cp .env.example .env

# To start the live backend which handles the realtime part
# (SERIES=f1 limits it to Formula 1, the default is every series)
cargo r -p realtime

# To start the api backend which handles the schedule
cargo r -p api
```

To develop against a past session, open it on the Replay page. With `ARCHIVE_DIR` pointing at a local copy of part of the F1 archive (`{year}/Index.json` and the session folders with their `.jsonStream` files), replays and tyre history work offline too.

You can also use the simulator and pass it a telemetry recording of a past race.

```bash
cd f1-dash/

# Start the simulator
cargo r -p simulator year-circuit.data.txt
```

You can find existing telemetry recordings [here](https://github.com/slowlydev/f1-dash-data-parser/releases/tag/data). If you want to record your own new sessions, here is how:

```bash
cd f1-dash/

# Start the saver and save the telemetry recording in the year-circuit.data.txt file
cargo r -p saver year-circuit.data.txt
```

> [!NOTE]
> I recommend naming the files with the ending 
> ".data.txt" as this extension is in the gitignore so you won't accidentally commit the telemetry recordings.

## Native app

The desktop and mobile apps are a [Tauri](https://v2.tauri.app) shell around the dashboard in `dashboard/src-tauri`. They run the feed adapters from `feeds/` in-process instead of talking to the realtime and api services. Install the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS first (on Linux: WebKitGTK 4.1 and friends).

```bash
cd dashboard

# desktop app with the dashboard's dev server and hot reloading
yarn tauri dev

# installers for the current OS into src-tauri/target/release/bundle
yarn tauri build

# a universal macOS build that runs on Apple Silicon and Intel
rustup target add aarch64-apple-darwin x86_64-apple-darwin
yarn tauri build --target universal-apple-darwin
```

The app bundles a static build of the dashboard (`NEXT_EXPORT=1`, set automatically when the Tauri CLI runs the build) and opens on `/dashboard/`.

### Mobile

The Android Studio and Xcode projects are generated rather than committed, so run `init` once per checkout. Android needs Android Studio with the SDK and NDK (set `ANDROID_HOME` and `NDK_HOME`); iOS needs macOS with Xcode.

```bash
yarn tauri android init
yarn tauri android dev      # emulator or a device over USB
yarn tauri android build --apk

yarn tauri ios init         # the app targets iPhone and iPad
yarn tauri ios dev
yarn tauri ios build
```

### Release builds

`.github/workflows/app.yaml` builds every platform on pull requests and drafts a release with the desktop installers when a tag like `app-v4.1.0` is pushed. Without signing keys it still produces working builds, with caveats:

- **Android**: a debug APK. For a signed release APK add the repository secrets `ANDROID_KEYSTORE` (base64 encoded keystore), `ANDROID_KEYSTORE_PASSWORD` and `ANDROID_KEY_ALIAS`.
- **iOS / iPadOS**: a simulator build. For an installable IPA add `IOS_CERTIFICATE`, `IOS_CERTIFICATE_PASSWORD`, `IOS_MOBILE_PROVISION` and `APPLE_DEVELOPMENT_TEAM`, as described in Tauri's [iOS code signing guide](https://v2.tauri.app/distribute/sign/ios/).
- **macOS**: unsigned, so Gatekeeper asks for confirmation on first launch. Signing and notarization follow Tauri's [macOS guide](https://v2.tauri.app/distribute/sign/macos/).
- **Windows**: unsigned, so SmartScreen may warn.

The bundle identifier is `io.github.alexgduarte.f1dash` in `dashboard/src-tauri/tauri.conf.json`; change it if you publish under another name.

## Adding a series

A series is an adapter in `feeds/src/adapters/` that fills a `Sink` with topics in the F1 live timing layout (`DriverList`, `TimingData`, `SessionInfo`, `TrackStatus`, ...), plus a schedule source in `feeds/src/schedule.rs`. Register it in `feeds/src/series.rs` and `feeds/src/adapters/mod.rs`, and in `dashboard/src/lib/series.ts` for the switcher. `feeds/src/adapters/util.rs` has a `Publisher` that only sends the topics that changed.

## Branching Convention

For branch names we use git flow style branching.

For new features follow this: `feature/the-name-of-the-feature`  
For a bugfix or refactor follow this: `bugfix/a-title-for-the-bugfix`

These feature and bugfix branches should be based off `develop` and be merged into `develop`.

## Commit Convention

For the commit message please use conventional commits:
[https://www.conventionalcommits.org/en/v1.0.0/](https://www.conventionalcommits.org/en/v1.0.0/)

### A Quick TL;DR; Of Conventional Commits

- `feat` When adding a new feature
- `fix` When fixing something
- `refactor` When it's neither a fix or a new feature
- `perf` If the change improves performance
- `chore` Anything else (should be last resort)

## Before opening a Pull Request

Please test your code, build the parts of the application you touched. For example, if you made changes in the frontend, make sure to run `yarn build` and see if the build succeeds and maybe check out how it will look in prod via `yarn start`. Sometimes there is a difference between running `dev` and `start` & `build`.

Make sure you format the files you created or touched. We use prettier for formatting, so either run the command `yarn run prettier` or install the fitting extension for your preferred IDE.

When opening a Pull Request please select `develop` as the target branch.