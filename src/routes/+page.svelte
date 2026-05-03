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
      if (!audioCtx) {
        audioCtx = new window.AudioContext({ sampleRate: 48000 });
      }
      const ctx = audioCtx;
      
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

  function auditEnvironment() {
    const check = (name: string) => !!(window as any)[name];
    log(`[AUDIT] WebCodecs classes: 
      VideoEncoder: ${check('VideoEncoder')}, 
      VideoDecoder: ${check('VideoDecoder')}, 
      AudioEncoder: ${check('AudioEncoder')}, 
      AudioDecoder: ${check('AudioDecoder')},
      EncodedVideoChunk: ${check('EncodedVideoChunk')},
      EncodedAudioChunk: ${check('EncodedAudioChunk')},
      VideoFrame: ${check('VideoFrame')},
      AudioData: ${check('AudioData')}`);
    
    if (typeof (window as any).VideoDecoder === 'undefined') {
      log('[ERR] VideoDecoder is NOT supported in this browser environment!');
    }
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
      
      // --- EVENT LISTENERS (Persistent) ---
      const { listen } = await import('@tauri-apps/api/event');
      
      // Clean up previous listeners if any (though usually we refresh the page)
      if ((window as any)._unlistenMedia) (window as any)._unlistenMedia();
      if ((window as any)._unlistenConnecting) (window as any)._unlistenConnecting();
      if ((window as any)._unlistenConnected) (window as any)._unlistenConnected();

      (window as any)._unlistenConnecting = await listen('webrtc-connecting', (event) => {
        log(`[RUST] Connecting: ${event.payload}`);
      });

      (window as any)._unlistenConnected = await listen('webrtc-connected', (event) => {
        connectionState = 'CONNECTED';
        log(`[RUST] Connected: ${event.payload}`);
      });

      auditEnvironment();

      // --- WEBCODECS DECODING SETUP ---
      const remoteCanvas = document.createElement('canvas');
      remoteCanvas.width = 640;
      remoteCanvas.height = 480;
      const remoteCtx = remoteCanvas.getContext('2d');
      const remoteCanvasStream = remoteCanvas.captureStream(30);
      
      function tryBindRemoteVideo() {
        if (remoteVideoRef) {
          remoteVideoRef.srcObject = remoteCanvasStream;
          remoteVideoRef.play().catch(e => log(`[WARN] video.play() failed: ${e}`));
          log('[OK] Remote video element bound and play() called');
          if (localStream) setupVisualizers(localStream, remoteVideoRef);
        } else {
          requestAnimationFrame(tryBindRemoteVideo);
        }
      }
      tryBindRemoteVideo();

      let videoDecoder: any = null;
      let remoteFrameCount = 0;
      let waitingForKeyframe = true;
      let videoTimestamp = 0;
      let decoderCrashCount = 0;

      function createVideoDecoder() {
        waitingForKeyframe = true;
        videoDecoder = new (window as any).VideoDecoder({
          output: (frame: any) => {
            remoteFrameCount++;
            if (remoteFrameCount === 1 || remoteFrameCount % 100 === 0) {
              log(`[DEC] Decoded frame #${remoteFrameCount} (${frame.displayWidth}x${frame.displayHeight})`);
            }
            if (remoteCtx) {
              remoteCtx.drawImage(frame, 0, 0, remoteCanvas.width, remoteCanvas.height);
            }
            frame.close();
          },
          error: (e: any) => {
            decoderCrashCount++;
            if (decoderCrashCount <= 5) {
              log(`[ERR] VideoDecoder FATAL #${decoderCrashCount}: ${e.message}`);
            }
            if (decoderCrashCount < 20) {
              setTimeout(() => createVideoDecoder(), 100);
            } else if (decoderCrashCount === 20) {
              log('[ERR] VideoDecoder has crashed 20 times. Giving up on H.264 decoding.');
            }
          }
        });
        videoDecoder.configure({ 
          codec: 'avc1.64001F', // High Profile, Level 3.1
          hardwareAcceleration: 'prefer-hardware'
        });
      }
      createVideoDecoder();

      // === SELF-TEST: Verify H.264 decode works at all in this browser ===
      try {
        const testCanvas = document.createElement('canvas');
        testCanvas.width = 64;
        testCanvas.height = 64;
        const tctx = testCanvas.getContext('2d')!;
        const grad = tctx.createLinearGradient(0, 0, 64, 64);
        grad.addColorStop(0, 'blue');
        grad.addColorStop(1, 'green');
        tctx.fillStyle = grad;
        tctx.fillRect(0, 0, 64, 64);

        const testFrame = new (window as any).VideoFrame(testCanvas, { timestamp: 0 });
        
        let selfTestDecoded = false;
        const testDecoder = new (window as any).VideoDecoder({
          output: (frame: any) => {
            selfTestDecoded = true;
            log(`[SELF-TEST] ✓ H.264 decode WORKS! Got ${frame.displayWidth}x${frame.displayHeight} frame.`);
            frame.close();
          },
          error: (e: any) => {
            log(`[SELF-TEST] ✗ H.264 decode FAILED: ${e.message}`);
          }
        });
        // Try both hardware and software for self-test to be sure
        testDecoder.configure({ codec: 'avc1.64001F', hardwareAcceleration: 'prefer-hardware' });

        const testEncoder = new (window as any).VideoEncoder({
          output: (chunk: any) => {
            // Prep Annex-B start code
            const annexB = new Uint8Array([0, 0, 0, 1]);
            const buf = new Uint8Array(chunk.byteLength + 4);
            buf.set(annexB, 0);
            chunk.copyTo(buf.subarray(4));

            try {
              testDecoder.decode(new (window as any).EncodedVideoChunk({
                type: chunk.type,
                timestamp: chunk.timestamp,
                data: buf
              }));
            } catch (e: any) {
              log(`[SELF-TEST] Decode call threw: ${e.name}: ${e.message}`);
            }
          },
          error: (e: any) => log(`[SELF-TEST] Encode error: ${e}`)
        });
        testEncoder.configure({ codec: 'avc1.64001F', width: 64, height: 64, bitrate: 500_000, latencyMode: 'realtime' });
        testEncoder.encode(testFrame, { keyFrame: true });
        testFrame.close();
        
        await testEncoder.flush();
        await testDecoder.flush();
        
        if (!selfTestDecoded) {
          log('[SELF-TEST] ✗ No H.264 decoded frame received after flush.');
        }
        
        testEncoder.close();
        testDecoder.close();
      } catch (e: any) {
        log(`[SELF-TEST] Failed to run: ${e.name}: ${e.message}`);
      }

      if (!audioCtx) {
        audioCtx = new window.AudioContext({ sampleRate: 48000 });
      }
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
      audioDecoder.configure({ codec: 'opus', sampleRate: 48000, numberOfChannels: 1 });

      let rxVideoChunks = 0;
      let rxAudioChunks = 0;

      // Check if H.264 is actually supported in this browser engine
      try {
        const support = await (window as any).VideoDecoder.isConfigSupported({ codec: 'avc1.64001F' });
        log(`[AUDIT] H.264 support: supported=${support.supported}`);
      } catch (e) {
        log(`[WARN] Could not check H.264 support: ${e}`);
      }

      if ((window as any)._unlistenMedia) (window as any)._unlistenMedia();
      (window as any)._unlistenMedia = await listen('webrtc-media-data', (event) => {
        const [kind, data] = event.payload as [string, number[]];
        const uint8 = new Uint8Array(data);
        if (uint8.length === 0) return;

        if (kind === 'video') {
          rxVideoChunks++;

          // H.264 doesn't have a simple 1-byte keyframe flag like VP8.
          // However, since we are controlling the encoder, we can rely on 
          // the fact that we're sending frames as we receive them.
          // For now, let's assume the first frame we get is a keyframe (or wait for one)
          
          if (rxVideoChunks === 1) {
            const head = Array.from(uint8.slice(0, 16)).map(b => b.toString(16).padStart(2, '0')).join(' ');
            log(`[PROBE] First H.264 frame: ${uint8.length} bytes. Hex: [${head}]`);
          }

          if (waitingForKeyframe) {
            waitingForKeyframe = false; 
            log(`[SYS] H.264 data arrived (${uint8.length} bytes). Starting decode...`);
          }

          try {
            if (videoDecoder && videoDecoder.state === 'configured') {
              videoTimestamp += 33333; 
              
              // Ensure Annex-B start code
              let finalData = uint8;
              if (uint8[0] !== 0 || uint8[1] !== 0 || uint8[2] !== 0 || uint8[3] !== 1) {
                finalData = new Uint8Array(uint8.length + 4);
                finalData.set([0, 0, 0, 1], 0);
                finalData.set(uint8, 4);
              }

              videoDecoder.decode(new (window as any).EncodedVideoChunk({
                type: 'key', // Force keyframe type for now
                timestamp: videoTimestamp,
                data: finalData
              }));
            }
          } catch (e: any) {
            log(`[ERR] H.264 Decode threw: ${e.name}: ${e.message}`);
          }
        } else if (kind === 'audio') {
          rxAudioChunks++;
          try {
            audioDecoder.decode(new (window as any).EncodedAudioChunk({
              type: 'key',
              timestamp: performance.now() * 1000,
              data: uint8
            }));
          } catch (e) {
            if (rxAudioChunks % 500 === 0) log(`[ERR] Audio decode fail: ${e}`);
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
            const annexB = new Uint8Array([0, 0, 0, 1]);
            const data = new Uint8Array(chunk.byteLength + 4);
            data.set(annexB, 0);
            chunk.copyTo(data.subarray(4));
            
            if (Math.random() < 0.01) log(`[TX] Sending ${data.length} byte H.264 frame`);
            invoke('send_video_chunk', { data: Array.from(data) });
          }
        },
        error: (e: any) => log(`[ERR] VideoEncoder: ${e}`)
      });
      try {
        videoEncoder.configure({ 
          codec: 'avc1.64001F', 
          width: 640, 
          height: 480, 
          bitrate: 1_000_000,
          latencyMode: 'realtime'
        });
        log('[OK] VideoEncoder configured for H.264 High Profile');
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
