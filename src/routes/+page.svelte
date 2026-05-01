<script lang="ts">
  import { invoke, Channel } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';

  let roomId = $state('FYP-DEMO-2026');
  let sigServer = $state('ws://127.0.0.1:8090');
  let connectionState = $state('DISCONNECTED');
  let logs: string[] = $state(['System Initialised... Ready.']);
  
  let localVideoRef: HTMLVideoElement | null = $state(null);
  let remoteVideoRef: HTMLVideoElement | null = $state(null);
  
  // Video transmission states
  let mediaRecorder: MediaRecorder | null = null;
  let sourceBuffer: SourceBuffer | null = null;
  let mediaSource: MediaSource | null = null;

  function log(msg: string) {
    logs = [...logs, msg];
  }

  async function connect() {
    connectionState = 'CONNECTING';
    log(`[SYS] establishing link to ${sigServer}...`);
    log(`[SYS] authenticating room key: ${roomId}`);
    
    try {
      // 1. Capture local webcam
      log('[SYS] Requesting webcam access...');
      let stream: MediaStream;
      try {
        stream = await navigator.mediaDevices.getUserMedia({ video: true, audio: false });
      } catch (err) {
        log(`[WARN] Camera access denied or unavailable: ${err}. Falling back to mocked canvas stream.`);
        const canvas = document.createElement('canvas');
        canvas.width = 640;
        canvas.height = 480;
        const ctx = canvas.getContext('2d')!;
        
        let colorOffset = 0;
        setInterval(() => {
          ctx.fillStyle = `hsl(${colorOffset % 360}, 100%, 50%)`;
          ctx.fillRect(0, 0, 640, 480);
          ctx.fillStyle = 'white';
          ctx.font = '30px monospace';
          ctx.fillText(`MOCK FRAME: ${colorOffset}`, 50, 240);
          colorOffset += 5;
        }, 33);
        
        stream = canvas.captureStream(30);
      }
      if (localVideoRef) {
        localVideoRef.srcObject = stream;
      }
      
      // 2. Setup WebM MediaRecorder to emit chunks
      const mimeTypes = [
        'video/webm;codecs=vp8',
        'video/webm;codecs=vp9',
        'video/webm',
        'video/mp4',
      ];
      const supportedMime = mimeTypes.find(m => MediaRecorder.isTypeSupported(m));
      
      if (supportedMime) {
        log(`[SYS] Using MediaRecorder codec: ${supportedMime}`);
        mediaRecorder = new MediaRecorder(stream, { mimeType: supportedMime });
        mediaRecorder.ondataavailable = async (e) => {
          if (e.data.size > 0 && connectionState === 'CONNECTED') {
            const buffer = await e.data.arrayBuffer();
            await invoke('send_video_chunk', { chunk: Array.from(new Uint8Array(buffer)) });
          }
        };
      } else {
        log('[WARN] No supported MediaRecorder codec found. Video will not be transmitted.');
      }
      
      // 3. Setup Remote MediaSource for incoming bytes
      mediaSource = new MediaSource();
      if (remoteVideoRef) {
        remoteVideoRef.src = URL.createObjectURL(mediaSource);
      }
      mediaSource.onsourceopen = () => {
        sourceBuffer = mediaSource!.addSourceBuffer('video/webm; codecs="vp8"');
      };
      
      // 4. Listen for incoming remote video WebM chunks from Rust
      const onRemoteChunk = new Channel<number[]>();
      onRemoteChunk.onmessage = (message) => {
        if (sourceBuffer && !sourceBuffer.updating) {
          sourceBuffer.appendBuffer(new Uint8Array(message));
        }
      };
      await invoke('subscribe_video', { onChunk: onRemoteChunk });

      // Actually invoke Rust WebRTC Start Command
      await invoke('start_rtc', { room: roomId, sigUrl: sigServer });
      
      setTimeout(() => {
        connectionState = 'CONNECTED';
        mediaRecorder?.start(100); // 100ms chunks
        log('[OK] ICE Gathering Complete.');
        log('[OK] WebRTC P2P Channel LIVE. Emitting video frames.');
      }, 1500);
    } catch (e) {
      log(`[ERR] ${e}`);
      connectionState = 'DISCONNECTED';
    }
  }

  const asciiLogo = `
  _____            __  __  _____ 
 |  __ \\     /\\   |  \\/  |/ ____|
 | |__) |   /  \\  | \\  / | (___  
 |  _  /   / /\\ \\ | |\\/| |\\___ \\ 
 | | \\ \\  / ____ \\| |  | |____) |
 |_|  \\_\\/_/    \\_\\_|  |_|_____/ 
                                 
 `;
</script>

<div class="container">
  <div class="retro-panel">
    <div class="ascii-art">{asciiLogo}</div>
    
    {#if connectionState === 'DISCONNECTED'}
      <div class="form-group">
        <label for="room">> ROOM_KEY</label>
        <input id="room" type="text" bind:value={roomId} spellcheck="false" />
        
        <label for="sig">> SIG_SERVER_IP</label>
        <input id="sig" type="text" bind:value={sigServer} spellcheck="false" />
        
        <button class="mt-4" onclick={connect}>[ INIT UPLINK ]</button>
      </div>
    {:else}
      <div class="video-grid">
        <div class="video-box local">
          <span>> LOCAL_TX [VP8]</span>
          <video bind:this={localVideoRef} autoplay muted playsinline class="glow"></video>
        </div>
        <div class="video-box remote">
          <span>> REMOTE_RX [VP8]</span>
          <video bind:this={remoteVideoRef} autoplay playsinline></video>
        </div>
      </div>
    {/if}

    <div class="terminal mt-4">
      {#each logs as l}
        <div>> {l}</div>
      {/each}
      {#if connectionState === 'CONNECTING'}
        <div class="blink">_</div>
      {/if}
    </div>
  </div>
</div>

<style>
  .container {
    width: 800px;
    max-width: 95vw;
  }
  
  .mt-4 { margin-top: 1rem; }
  
  .form-group label {
    display: block;
    color: var(--text-muted);
    font-size: 0.8rem;
    margin-bottom: 0.5rem;
  }
  
  .video-grid {
    display: flex;
    gap: 1.5rem;
  }
  
  .video-box {
    border: 1px dashed var(--border-color);
    flex: 1;
    display: flex;
    flex-direction: column;
    padding: 0.5rem;
    position: relative;
  }
  
  .video-box span {
    font-size: 0.8rem;
    color: var(--text-muted);
    margin-bottom: 0.5rem;
    border-bottom: 1px dashed var(--border-color);
    padding-bottom: 0.2rem;
    display: block;
  }
  
  video {
    width: 100%;
    height: 250px;
    background: #050505;
    object-fit: cover;
  }
  
  video.glow {
    color: var(--primary-glow);
    text-shadow: 0 0 5px rgba(0, 255, 65, 0.4);
  }
  
  .terminal {
    height: 120px;
    background: #050505;
    border: 1px dashed var(--border-color);
    padding: 0.8rem;
    font-size: 0.8rem;
    overflow-y: auto;
    color: var(--text-muted);
    display: flex;
    flex-direction: column;
  }
  
  .blink {
    animation: blinker 1s linear infinite;
    color: var(--primary-glow);
  }
  
  @keyframes blinker {
    50% { opacity: 0; }
  }
</style>
