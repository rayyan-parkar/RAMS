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
      await invoke('close_call');
      log('[OK] Call closed.');
    } catch (e) {
      log(`[WARN] Failed to gracefully close call: ${e}`);
    }
    
    window.location.reload();
  }

  async function connect() {
    connectionState = 'CONNECTING';
    log(`[SYS] establishing link to ${sigServer}...`);
    log(`[SYS] authenticating room key: ${roomId}`);
    
    try {
      log(`[SYS] WebCodecs Check: VideoEncoder=${!!(window as any).VideoEncoder}, AudioEncoder=${!!(window as any).AudioEncoder}`);
      
      const { listen } = await import('@tauri-apps/api/event');

      await listen('webrtc-connecting', (event) => {
        log(`[RUST] Connecting: ${event.payload}`);
      });

      await listen('webrtc-connected', (event) => {
        connectionState = 'CONNECTED';
        log(`[RUST] Connected: ${event.payload}`);
      });

      // --- WEBCODECS DECODING SETUP ---
      const remoteCanvas = document.createElement('canvas');
      remoteCanvas.width = 640;
      remoteCanvas.height = 480;
      const remoteCtx = remoteCanvas.getContext('2d');
      const remoteCanvasStream = remoteCanvas.captureStream(30);
      
      // Defer binding until DOM element is available (Svelte bind:this runs after tick)
      function tryBindRemoteVideo() {
        if (remoteVideoRef) {
          remoteVideoRef.srcObject = remoteCanvasStream;
          log('[OK] Remote video element bound to decode canvas');
        } else {
          log('[SYS] Waiting for remote video element to mount...');
          requestAnimationFrame(tryBindRemoteVideo);
        }
      }
      tryBindRemoteVideo();

      const videoDecoder = new (window as any).VideoDecoder({
        output: (frame: any) => {
          if (remoteCtx) {
            remoteCtx.drawImage(frame, 0, 0, remoteCanvas.width, remoteCanvas.height);
          }
          frame.close();
        },
        error: (e: any) => log(`[ERR] VideoDecoder: ${e}`)
      });
      try {
        videoDecoder.configure({ codec: 'vp8' });
        log('[OK] VideoDecoder configured for VP8');
      } catch (e) {
        log(`[ERR] VideoDecoder config failed: ${e}`);
      }

      audioCtx = new window.AudioContext({ sampleRate: 48000 });
      const audioDecoder = new (window as any).AudioDecoder({
        output: (audioData: any) => {
          if (!audioCtx) return;
          const buffer = audioCtx.createBuffer(1, audioData.numberOfFrames, audioData.sampleRate);
          const channelData = new Float32Array(audioData.numberOfFrames);
          audioData.copyTo(channelData, { planeIndex: 0 });
          buffer.copyToChannel(channelData, 0);

          const source = audioCtx.createBufferSource();
          source.buffer = buffer;
          source.connect(audioCtx.destination);
          source.start();
          audioData.close();
        },
        error: (e: any) => log(`[ERR] AudioDecoder: ${e}`)
      });
      try {
        audioDecoder.configure({ codec: 'opus', sampleRate: 48000, numberOfChannels: 1 });
        log('[OK] AudioDecoder configured for Opus (1-ch)');
      } catch (e) {
        log(`[ERR] AudioDecoder config failed: ${e}`);
      }

      // Ensure AudioContext is active
      if (audioCtx.state === 'suspended') {
        audioCtx.resume();
      }

      await listen('webrtc-media-data', (event) => {
        const [kind, data] = event.payload as [string, number[]];
        const uint8 = new Uint8Array(data);
        if (uint8.length === 0) return;

        if (Math.random() < 0.01) log(`[RX] Received ${uint8.length} bytes of ${kind}`);

        if (kind === 'video') {
          // Basic VP8 keyframe check: first bit of payload descriptor is 0
          const isKeyframe = (uint8[0] & 0x01) === 0;
          try {
            const EncodedVideoChunkCtor = (window as any).EncodedVideoChunk;
            videoDecoder.decode(new EncodedVideoChunkCtor({
              type: isKeyframe ? 'key' : 'delta',
              timestamp: performance.now() * 1000,
              data: uint8
            }));
          } catch (e) {
            log(`[ERR] Video decode fail: ${e}`);
          }
        } else if (kind === 'audio') {
          try {
            const EncodedAudioChunkCtor = (window as any).EncodedAudioChunk;
            audioDecoder.decode(new EncodedAudioChunkCtor({
              type: 'key',
              timestamp: performance.now() * 1000,
              data: uint8
            }));
          } catch (e) {
            log(`[ERR] Audio decode fail: ${e}`);
          }
        }
      });
      // --------------------------------

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
      
      // --- WEBCODECS ENCODING SETUP ---
      const localCanvas = document.createElement('canvas');
      localCanvas.width = 640;
      localCanvas.height = 480;
      const localCtx = localCanvas.getContext('2d');
      
      const videoEncoder = new (window as any).VideoEncoder({
        output: (chunk: any) => {
          if (connectionState === 'CONNECTED') {
            const data = new Uint8Array(chunk.byteLength);
            chunk.copyTo(data);
            if (Math.random() < 0.01) log(`[TX] Sending ${data.length} byte video frame`);
            invoke('send_video_chunk', { data: Array.from(data) });
          }
        },
        error: (e: any) => log(`[ERR] VideoEncoder: ${e}`)
      });
      try {
        videoEncoder.configure({ codec: 'vp8', width: 640, height: 480, bitrate: 1_000_000 });
        log('[OK] VideoEncoder configured');
      } catch (e) {
        log(`[ERR] VideoEncoder config failed: ${e}`);
      }

      let frameCount = 0;
      function encodeVideo() {
        if (localVideoRef && localVideoRef.readyState >= 2 && localCtx) {
          localCtx.drawImage(localVideoRef, 0, 0, localCanvas.width, localCanvas.height);
          const frame = new (window as any).VideoFrame(localCanvas, { timestamp: performance.now() * 1000 });
          videoEncoder.encode(frame, { keyFrame: (frameCount % 30 === 0) });
          frame.close();
          frameCount++;
        }
        requestAnimationFrame(encodeVideo);
      }
      encodeVideo(); // Start grabbing frames

      // Audio Encoding (force 48kHz to match Opus encoder config)
      const captureAudioCtx = new window.AudioContext({ sampleRate: 48000 });
      const audioSource = captureAudioCtx.createMediaStreamSource(stream);
      // Using ScriptProcessorNode (deprecated but widely supported) to grab PCM data
      const scriptNode = captureAudioCtx.createScriptProcessor(4096, 1, 1);
      
      const audioEncoder = new (window as any).AudioEncoder({
        output: (chunk: any) => {
          if (connectionState === 'CONNECTED') {
            const data = new Uint8Array(chunk.byteLength);
            chunk.copyTo(data);
            if (Math.random() < 0.01) log(`[TX] Sending ${data.length} byte audio frame`);
            invoke('send_audio_chunk', { data: Array.from(data) });
          }
        },
        error: (e: any) => log(`[ERR] AudioEncoder: ${e}`)
      });
      try {
        audioEncoder.configure({ codec: 'opus', sampleRate: 48000, numberOfChannels: 1, bitrate: 64000 });
        log('[OK] AudioEncoder configured');
      } catch (e) {
        log(`[ERR] AudioEncoder config failed: ${e}`);
      }

      let audioTime = 0;
      scriptNode.onaudioprocess = (e) => {
        if (connectionState !== 'CONNECTED') return;
        const pcm = e.inputBuffer.getChannelData(0);
        const audioData = new (window as any).AudioData({
          format: 'f32-planar',
          sampleRate: 48000,
          numberOfFrames: pcm.length,
          numberOfChannels: 1,
          timestamp: audioTime,
          data: pcm
        });
        audioTime += (pcm.length / 48000) * 1000000;
        audioEncoder.encode(audioData);
        audioData.close();
      };
      audioSource.connect(scriptNode);
      scriptNode.connect(captureAudioCtx.destination);
      // --------------------------------
      
      // 4. Listen for incoming remote video
      // We will re-implement this using RTP soon
      log('[SYS] Preparing for RTP media routing...');

      // Actually invoke Rust WebRTC Start Command
      log('[SYS] Calling Rust start_quick_call now...');
      await invoke('start_quick_call', { roomId, wsUrl: sigServer });
      log(`[SYS] Invoked start_quick_call for room: ${roomId}`);
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
