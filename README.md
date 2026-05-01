# Tauri + SvelteKit + TypeScript

This template should help get you started developing with Tauri, SvelteKit and TypeScript in Vite.

## Usage

Install dependencies:

```bash
bun install
```

Run the web app (no Tauri):

```bash
bun run dev
```

Run the Tauri app (Linux/X11):

```bash
WEBKIT_DISABLE_COMPOSITING_MODE=1 GDK_BACKEND=x11 bun run tauri dev
```

For now, this is intended to test over localhost only.

## Linux camera dependencies (WebKitGTK + GStreamer)

The Tauri WebView on Linux uses WebKitGTK, which relies on GStreamer for camera capture.
If you see errors like **appsink/autoaudiosink not found**, install these packages:

**Ubuntu / Debian**
```bash
sudo apt update
sudo apt install gstreamer1.0-tools gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly gstreamer1.0-libav
```

**Fedora**
```bash
sudo dnf install gstreamer1 gstreamer1-plugins-base gstreamer1-plugins-good gstreamer1-plugins-bad-free gstreamer1-plugins-ugly-free gstreamer1-libav
```

**Arch**
```bash
sudo pacman -S gstreamer gst-plugins-base gst-plugins-good gst-plugins-bad gst-plugins-ugly gst-libav
```

**openSUSE**
```bash
sudo zypper install gstreamer gstreamer-plugins-base gstreamer-plugins-good gstreamer-plugins-bad gstreamer-plugins-ugly gstreamer-plugins-libav
```

## Localhost room-code testing

The UI exposes two fields:

- `ROOM_KEY`: the room code both peers must share
- `SIG_SERVER_IP`: the WebSocket URL of your signaling server (defaults to `ws://127.0.0.1:8080`)

To connect **two devices on your LAN**:

1. Run your signaling server on one machine (not included in this repo) and bind it to `0.0.0.0:8080`.
2. On **both** devices, enter the same `ROOM_KEY`.
3. Set `SIG_SERVER_IP` to `ws://<HOST_LAN_IP>:8080` on both devices.
4. Press `[ INIT UPLINK ]` on both devices.

If both peers are on the **same machine**, keep the default `ws://127.0.0.1:8080` and use the same `ROOM_KEY`.

## RAMS WebRTC Framework

A high-performance sans-IO WebRTC framework designed in Rust leveraging the strictly-typed `str0m` stack, integrated into a desktop application via Tauri and Svelte.

## Running Locally

To test the WebRTC Live Video pipeline across identical network clients over P2P UDP:

1. **Start the Signaling Server**
   ```bash
   cd signaling-server
   bun install
   bun run start
   ```

2. **Run the RAMS Application**
   ```bash
   bun install
   bun run tauri dev
   ```

3. **Establish connection**
   Press **[ INIT UPLINK ]** on both running instances across your local network (ensure you update the UI's `ws://127.0.0.1:8090` IP to your machine's LAN IP if running on separate devices).
   
   The local webcam preview should instantly bind, and RTP network video should begin streaming securely through your UDP sockets!

## Recommended IDE Setup

[VS Code](https://code.visualstudio.com/) + [Svelte](https://marketplace.visualstudio.com/items?itemName=svelte.svelte-vscode) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer).
