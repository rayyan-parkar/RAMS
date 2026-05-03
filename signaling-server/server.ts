import { WebSocketServer, WebSocket } from 'ws';

const wss = new WebSocketServer({ port: 8090, host: '0.0.0.0' });
console.log('RAMS Signaling Server running on ws://localhost:8090');

// room_id -> Set of connected WebSockets
const rooms = new Map<string, Set<WebSocket>>();

wss.on('connection', (ws: WebSocket) => {
    let currentRoom: string | null = null;
    console.log('[+] Client connected');

    ws.on('message', (data: string | Uint8Array | ArrayBuffer) => {
        try {
            const raw = typeof data === 'string'
                ? data
                : new TextDecoder().decode(data instanceof ArrayBuffer ? new Uint8Array(data) : data);
            const msg = JSON.parse(raw);
            console.log(`[>>] Incoming websocket message: ${msg.type || 'unknown'}`);

            // Rust sends: {"type":"join","room":"..."} via serde(tag = "type", rename_all = "camelCase")
            if (msg.type === "join" && msg.room) {
                currentRoom = msg.room;
                if (!rooms.has(currentRoom!)) {
                    rooms.set(currentRoom!, new Set());
                }
                const room = rooms.get(currentRoom!)!;
                const isInitiator = room.size === 0; // First peer is initiator
                room.add(ws);
                console.log(`[+] Client joined room: ${currentRoom} (initiator: ${isInitiator}, peers: ${room.size})`);

                // Send back a Joined message matching Rust's SignalingMessage::Joined
                // NOTE: Rust uses #[serde(rename_all = "camelCase")] so field must be "isInitiator"
                ws.send(JSON.stringify({
                    type: "joined",
                    room: currentRoom,
                    isInitiator: isInitiator,
                }));
                console.log(`[<<] Sent joined response to ${isInitiator ? 'initiator' : 'responder'} for room ${currentRoom}`);

                // Notify existing peers that someone joined
                if (!isInitiator) {
                    for (const client of room) {
                        if (client !== ws && client.readyState === WebSocket.OPEN) {
                            console.log(`[>>] Notifying peerJoined to room ${currentRoom}`);
                            client.send(JSON.stringify({ type: "peerJoined" }));
                        }
                    }
                }
                return;
            }

            // Forward offer/answer/iceCandidate to other peers in the room
            if (currentRoom && rooms.has(currentRoom)) {
                const actionType = msg.type || "unknown";
                console.log(`[>>] Routing ${actionType} in room ${currentRoom}`);

                for (const client of rooms.get(currentRoom)!) {
                    if (client !== ws && client.readyState === WebSocket.OPEN) {
                        console.log(`[>>] Forwarded ${actionType} to peer in room ${currentRoom}`);
                        client.send(JSON.stringify(msg));
                    }
                }
            } else {
                console.log('[!!] Received signaling message before room join or for missing room');
            }

        } catch (e) {
            console.error("Failed to parse incoming message:", data, e);
        }
    });

    ws.on('close', () => {
        if (currentRoom && rooms.has(currentRoom)) {
            rooms.get(currentRoom)!.delete(ws);
            console.log(`[-] Client disconnected from room: ${currentRoom}`);
            if (rooms.get(currentRoom)!.size === 0) {
                rooms.delete(currentRoom);
                console.log(`[ ] Destroyed empty room: ${currentRoom}`);
            }
        } else {
            console.log('[-] Unassigned client disconnected');
        }
    });
});
