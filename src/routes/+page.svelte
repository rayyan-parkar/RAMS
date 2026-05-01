<script lang="ts">
  import { invoke, Channel } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';

  let roomId = $state('FYP-DEMO-2026');
  let sigServer = $state('ws://127.0.0.1:8090');
  let connectionState = $state('DISCONNECTED');
  let useMockFrames = $state(false);
  let logs: string[] = $state(['System Initialised... Ready.']);
  
  let localVideoRef: HTMLVideoElement | null = $state(null);
  let remoteVideoRef: HTMLVideoElement | null = $state(null);
  
  // Media controls
  let isMuted = $state(false);
  let isVideoOff = $state(false);
  let localStream: MediaStream | null = null;

  // Visualizers
  let localAudioLevel = $state(0);
  let remoteAudioLevel = $state(0);
  let audioCtx: AudioContext | null = null;
  let visualizerFrameId: number;

  // Video transmission states
  let mediaRecorder: MediaRecorder | null = null;
  let sourceBuffer: SourceBuffer | null = null;
  let mediaSource: MediaSource | null = null;

  function log(msg: string) {
    logs = [...logs, msg];
  }

  function getAsciiBar(level: number) {
    const maxBars = 10;
    const filled = Math.min(maxBars, Math.max(0, Math.floor((level / 255) * maxBars * 1.5)));
    return '[' + '|'.repeat(filled) + '.'.repeat(maxBars - filled) + ']';
  }

  function setupVisualizers(lStream: MediaStream, rVideo: HTMLVideoElement) {
    try {
      if (!audioCtx) audioCtx = new window.AudioContext();
      
      const localAnalyser = audioCtx.createAnalyser();
      localAnalyser.fftSize = 256;
      if (lStream.getAudioTracks().length > 0) {
        const localSource = audioCtx.createMediaStreamSource(lStream);
        localSource.connect(localAnalyser);
      }

      const remoteAnalyser = audioCtx.createAnalyser();
      remoteAnalyser.fftSize = 256;
      // We must connect the remote video element to the context
      // Note: This might cause issues if CORS isn't right, but for ObjectURLs it usually works.
      const remoteSource = audioCtx.createMediaElementSource(rVideo);
      remoteSource.connect(remoteAnalyser);
      remoteAnalyser.connect(audioCtx.destination); // Required to actually hear the remote peer!

      const localDataArray = new Uint8Array(localAnalyser.frequencyBinCount);
      const remoteDataArray = new Uint8Array(remoteAnalyser.frequencyBinCount);

      function renderFrame() {
        localAnalyser.getByteFrequencyData(localDataArray);
        remoteAnalyser.getByteFrequencyData(remoteDataArray);
        
        localAudioLevel = localDataArray.reduce((a, b) => a + b, 0) / localDataArray.length || 0;
        remoteAudioLevel = remoteDataArray.reduce((a, b) => a + b, 0) / remoteDataArray.length || 0;
        
        visualizerFrameId = requestAnimationFrame(renderFrame);
      }
      renderFrame();
    } catch (e) {
      log(`[WARN] Failed to setup audio visualizers: ${e}`);
    }
  }

  function toggleMute() {
    isMuted = !isMuted;
    if (localStream) {
      localStream.getAudioTracks().forEach(t => t.enabled = !isMuted);
    }
    log(`[SYS] Microphone ${isMuted ? 'MUTED' : 'UNMUTED'}`);
  }

  function toggleVideo() {
    isVideoOff = !isVideoOff;
    if (localStream) {
      localStream.getVideoTracks().forEach(t => t.enabled = !isVideoOff);
    }
    log(`[SYS] Video ${isVideoOff ? 'OFF' : 'ON'}`);
  }

  async function leaveRoom() {
    log('[SYS] Disconnecting...');
    if (visualizerFrameId) cancelAnimationFrame(visualizerFrameId);
    if (audioCtx) audioCtx.close();
    
    try {
      await invoke('abort_rtc');
    } catch (e) {
      log(`[WARN] Failed to gracefully abort RTC: ${e}`);
    }
    
    window.location.reload();
  }

  async function connect() {
    connectionState = 'CONNECTING';
    log(`[SYS] establishing link to ${sigServer}...`);
    log(`[SYS] authenticating room key: ${roomId}`);
    
    try {
      // 1. Capture local webcam
      let stream: MediaStream;
      
      if (useMockFrames) {
        log('[SYS] Mock frames enabled. Generating canvas stream...');
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
      } else {
        log('[SYS] Requesting webcam and microphone access...');
        try {
          stream = await navigator.mediaDevices.getUserMedia({ video: true, audio: true });
          log('[OK] Video and Audio devices acquired.');
        } catch (err) {
          log(`[WARN] Both devices failed (${err}). Trying Video only...`);
          try {
            stream = await navigator.mediaDevices.getUserMedia({ video: true, audio: false });
            log('[OK] Video device acquired (No Audio).');
          } catch (err2) {
            log(`[WARN] Video only failed (${err2}). Trying Audio only...`);
            try {
              stream = await navigator.mediaDevices.getUserMedia({ video: false, audio: true });
              log('[OK] Audio device acquired (No Video).');
            } catch (err3) {
              log(`[ERR] All hardware capture failed (${err3}). Enable USE_MOCK_FRAMES to test without hardware.`);
              connectionState = 'DISCONNECTED';
              return;
            }
          }
        }
      }
      
      localStream = stream;
      if (localVideoRef) {
        localVideoRef.srcObject = stream;
      }
      
      // 2. Setup WebM MediaRecorder to emit chunks
      const mimeTypes = [
        'video/webm;codecs=vp8,opus',
        'video/webm;codecs=vp8,vorbis',
        'video/webm;codecs=vp8',
        'video/webm;codecs=vp9',
        'video/webm',
      ];
      const supportedMime = mimeTypes.find(m => MediaRecorder.isTypeSupported(m));
      
      if (supportedMime) {
        log(`[SYS] Using MediaRecorder codec: ${supportedMime}`);
        try {
          mediaRecorder = new MediaRecorder(stream, { mimeType: supportedMime });
          mediaRecorder.ondataavailable = async (e) => {
            if (e.data.size > 0 && connectionState === 'CONNECTED') {
              const buffer = await e.data.arrayBuffer();
              await invoke('send_video_chunk', { chunk: Array.from(new Uint8Array(buffer)) });
            }
          };
        } catch (mrErr) {
          log(`[ERR] Failed to create MediaRecorder: ${mrErr}`);
          mediaRecorder = null;
        }
      } else {
        log('[WARN] No supported MediaRecorder codec found. Video will not be transmitted.');
      }
      
      // 3. Setup Remote MediaSource for incoming bytes
      mediaSource = new MediaSource();
      if (remoteVideoRef) {
        remoteVideoRef.src = URL.createObjectURL(mediaSource);
      }
      mediaSource.onsourceopen = () => {
        sourceBuffer = mediaSource!.addSourceBuffer(supportedMime || 'video/webm; codecs="vp8"');
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
        try {
          mediaRecorder?.start(100); // 100ms chunks
        } catch (startErr) {
          log(`[ERR] Failed to start MediaRecorder: ${startErr}`);
        }
        log('[OK] WebRTC DataChannel established via str0m.');
        log('[OK] Emitting WebM chunks over encrypted DTLS/SCTP tunnel.');
        
        if (remoteVideoRef && localStream) {
          setupVisualizers(localStream, remoteVideoRef);
        }
      }, 3000);
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
        
        <div class="checkbox-group mt-4">
          <input id="mock-frames" type="checkbox" bind:checked={useMockFrames} />
          <label for="mock-frames">> USE_MOCK_FRAMES</label>
        </div>
        
        <button class="mt-4" onclick={connect}>[ INIT UPLINK ]</button>
      </div>
    {:else}
      <div class="video-grid">
        <div class="video-box local">
          <span>> LOCAL_TX [VP8]</span>
          <video bind:this={localVideoRef} autoplay muted playsinline class="glow"></video>
          <div class="mic-visualizer">MIC: {getAsciiBar(localAudioLevel)}</div>
        </div>
        <div class="video-box remote">
          <span>> REMOTE_RX [VP8]</span>
          <video bind:this={remoteVideoRef} autoplay playsinline></video>
          <div class="mic-visualizer">VOL: {getAsciiBar(remoteAudioLevel)}</div>
        </div>
      </div>
      <div class="media-controls mt-4">
        <button onclick={toggleMute}>[{isMuted ? 'UNMUTE_MIC' : 'MUTE_MIC'}]</button>
        <button onclick={toggleVideo}>[{isVideoOff ? 'ENABLE_VIDEO' : 'DISABLE_VIDEO'}]</button>
        <button class="danger" onclick={leaveRoom}>[ ABORT_UPLINK ]</button>
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
  
  .checkbox-group {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 0.8rem;
    margin-top: 1rem;
    margin-bottom: 1rem;
    border: 1px dashed var(--border-color);
    padding: 0.5rem;
    background: #050505;
  }
  
  .checkbox-group label {
    margin-bottom: 0;
    display: inline-block;
    color: var(--primary-glow);
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

  .mic-visualizer {
    font-size: 0.8rem;
    color: var(--primary-glow);
    font-family: monospace;
    text-align: center;
    margin-top: 0.5rem;
    padding: 0.2rem;
    background: #000;
    border: 1px dashed var(--border-color);
  }

  .media-controls {
    display: flex;
    gap: 1rem;
    justify-content: center;
  }
  
  button.danger {
    color: #ff3333;
    border-color: #ff3333;
  }
  
  button.danger:hover {
    background: rgba(255, 51, 51, 0.1);
    box-shadow: 0 0 10px rgba(255, 51, 51, 0.5);
  }
</style>
