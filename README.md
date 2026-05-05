# RAMS

A high-performance, **Sans-IO** WebRTC implementation built with Rust, Tauri, and Svelte. RAMS is designed as a tiered framework, allowing developers to use a low-level RTC core or a high-level automated orchestrator. It also includes a demo Svelte/Tauri app that uses the framework to create a live video conferencing app.

## Architecture

- **Rust Backend**: Leverages `str0m` as its internal state machine and extends it to add support for signaling using the ramscore struct as the primary way to manage the WebRTC state machine.
- **Tiered Design**: 
  - **Core**: Protocol-level WebRTC logic and signaling state management.
  - **Quick**: Automated ICE/STUN discovery and signaling orchestration via WebSockets.
- **Frontend**: Svelte 5 with a modular component architecture (`Terminal`, `VideoBox`) and a custom H.264 bitstream utility.
- **IPC Bridge**: High-frequency binary bridge for low-latency media transmission between WebKitGTK and Rust.

Note: There is no support for TURN servers added to this project, it was developed for my Final Year University Project, and is NOT intended for production use in its current state, however (assuming university regulations allow it, and to the best of my knowledge, they do) feel free to use and modify it for your own purposes, or submit Pull Requests, open Issues, etc. !


---

## Getting Started

### Nix / NixOS

If you have the [Nix package manager](https://nixos.org/download.html) installed, all dependencies (Rust, Node, GStreamer, WebKitGTK) are managed automatically via the direnv.

```bash
# Enter the development environment
nix develop
# OR if using direnv
direnv allow
```
After you enable the direnv, it will automatically load all required dependencies for you!
### Non-Nix Users

You must manually install the following system dependencies:

1. **Rust & Bun**: [rustup.rs](https://rustup.rs/) and [bun.sh](https://bun.sh/).
2. **WebKitGTK**: Required for the Tauri WebView.
3. **GStreamer**: Required by WebKitGTK for camera and audio access.

#### Dependency Commands

**Ubuntu / Debian**
```bash
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libssl-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev \
gstreamer1.0-tools gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly gstreamer1.0-libav
```

**Fedora**
```bash
sudo dnf install webkit2gtk4.1-devel openssl-devel gtk3-devel libappindicator-gtk3-devel librsvg2-devel \
gstreamer1 gstreamer1-plugins-base gstreamer1-plugins-good gstreamer1-plugins-bad-free gstreamer1-plugins-ugly-free gstreamer1-libav
```

**Arch Linux**
```bash
sudo pacman -S webkit2gtk-4.1 base-devel curl wget openssl gtk3 libappindicator-gtk3 librsvg \
gstreamer gst-plugins-base gst-plugins-good gst-plugins-bad gst-plugins-ugly gst-libav
```

---

## Usage Instructions

### 1. Start the Signaling Server
The signaling server handles peer discovery and role assignment.
```bash
cd signaling-server
bun install
bun run start # Starts on ws://127.0.0.1:8090 by default
```

Note: You can run the signaling server, and forward the port as long as you change the signaling server IP on each client to match the public IP of the server, or alternatively use tailscale to create a virtual LAN to connect to your peer. 

### 2. Run the RAMS Application
Launch the desktop client.
```bash
bun install
bun run tauri dev
```

### 3. Connect
1. Ensure both peers (or two instances on one machine) have the same **Room Key**.
2. Press **[ INIT UPLINK ]** on both.
3. The terminal will track the ICE handshake, DTLS completion, and media flow.

---

## Testing

RAMS includes unit tests covering protocol parsing, state machine transitions, and core media orchestration.

```bash
# Run all tests
cd src-tauri
cargo test
```

Additionally, when run with `bun tauri dev` the client includes a very detailed set of logs in the event something goes wrong.

- **Note for Linux Users**: If the UI is being strange or flickers, especially on Hyprland or forks of it, try running with: `WEBKIT_DISABLE_COMPOSITING_MODE=1 GDK_BACKEND=x11 bun run tauri dev`.

### Features
The backend can be compiled with specific features:
- **`default`**: Full suite (Core + Quick).
- **`core`**: Lightweight build, removing the automated WebSocket orchestrator and its dependencies (`tokio`, `tungstenite`).

```bash
# Build only the core library
cargo build --no-default-features --features core
```

---

## License & Credits

This project was created for educational purposes and is not intended for production use. It uses the str0m crate, and various others which are all included within the Cargo.toml file. If you have an issue with any of the code included here, please open an issue.

The full-screen icon is provided by Lucide Icons (https://lucide.dev/)

A big thank you to the str0m team (https://github.com/fmilliseconds/str0m) for their wonderful work, glorious documentation and to WebRTC.rs team (https://github.com/webrtc-rs/webrtc) for their wonderful work as well, as well as my supervisor for their guidance during the creation of this project.

https://mit-license.org/