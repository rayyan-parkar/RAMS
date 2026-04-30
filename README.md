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

## Recommended IDE Setup

[VS Code](https://code.visualstudio.com/) + [Svelte](https://marketplace.visualstudio.com/items?itemName=svelte.svelte-vscode) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer).
