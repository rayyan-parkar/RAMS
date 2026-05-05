<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import * as h264 from '$lib/h264';
  import Terminal from '$lib/components/Terminal.svelte';
  import VideoBox from '$lib/components/VideoBox.svelte';

  let roomId = $state('FYP-DEMO-2026');
  let sigServer = $state('ws://127.0.0.1:8090');
  let connectionState = $state('DISCONNECTED');
  let logs: string[] = $state(['System Initialised... Ready.']);
  
  let localVideoRef: HTMLVideoElement | null = $state(null);
  let remoteVideoRef: HTMLVideoElement | null = $state(null);
  
  // Media controls
  let isMuted = $state(false);
  let isVideoOff = $state(false);
  let localStream: MediaStream | null = null;

  // Fullscreen mode
  let fullscreenTarget = $state<'local' | 'remote' | null>(null);
  let isFullscreen = $derived(fullscreenTarget !== null);
  let showExitButton = $state(false);
  let exitButtonTimeout: ReturnType<typeof setTimeout> | null = null;
  
  // Visualizers
  let localAudioLevel = $state(0);
  let remoteAudioLevel = $state(0);
  let audioCtx: AudioContext | null = null;
  let remoteAnalyser: AnalyserNode | null = null;
  let remoteDest: MediaStreamAudioDestinationNode | null = null;
  let debugAudioEl: HTMLAudioElement | null = null;
  let remoteGain: GainNode | null = null;
  let visualizerFrameId: number;

  function toggleFullscreen(target: 'local' | 'remote' | null) {
    if (target === null || fullscreenTarget === target) {
      fullscreenTarget = null;
    } else {
      fullscreenTarget = target;
    }
    if (exitButtonTimeout) clearTimeout(exitButtonTimeout);
    if (!fullscreenTarget) {
      showExitButton = false;
    }
  }

  function handleMouseMove() {
    if (isFullscreen) {
      showExitButton = true;
      if (exitButtonTimeout) clearTimeout(exitButtonTimeout);
      exitButtonTimeout = setTimeout(() => {
        showExitButton = false;
      }, 3000);
    }
  }

  function log(msg: string) {
    logs = [...logs, msg];
  }

  function setupVisualizers(lStream: MediaStream, rVideo: HTMLVideoElement) {
    try {
      if (!audioCtx) audioCtx = new window.AudioContext({ sampleRate: 48000 });
      const localAnalyser = audioCtx.createAnalyser();
      localAnalyser.fftSize = 256;
      if (lStream.getAudioTracks().length > 0) {
        const localSource = audioCtx.createMediaStreamSource(lStream);
        localSource.connect(localAnalyser);
      }

      if (!remoteAnalyser) {
        remoteAnalyser = audioCtx.createAnalyser();
        remoteAnalyser.fftSize = 256;
        if (!remoteGain) remoteGain = audioCtx.createGain();
        remoteGain.gain.value = 1.0;
        if (!remoteDest) {
          remoteDest = audioCtx.createMediaStreamDestination();
          debugAudioEl = document.createElement('audio');
          debugAudioEl.autoplay = true;
          debugAudioEl.muted = false;
          debugAudioEl.style.display = 'none';
          debugAudioEl.srcObject = remoteDest.stream;
          document.body.appendChild(debugAudioEl);
        }
        remoteAnalyser.connect(remoteGain);
        try { remoteGain.connect(audioCtx.destination); } catch (e) {}
        if (remoteDest) { try { remoteGain.connect(remoteDest); } catch (e) {} }
      }

      const localDataArray = new Uint8Array(localAnalyser.frequencyBinCount);
      const remoteDataArray = new Uint8Array(remoteAnalyser.frequencyBinCount);

      function renderFrame() {
        localAnalyser.getByteFrequencyData(localDataArray);
        if (remoteAnalyser) remoteAnalyser.getByteFrequencyData(remoteDataArray);
        localAudioLevel = localDataArray.reduce((a, b) => a + b, 0) / localDataArray.length || 0;
        remoteAudioLevel = remoteDataArray.reduce((a, b) => a + b, 0) / remoteDataArray.length || 0;
        visualizerFrameId = requestAnimationFrame(renderFrame);
      }
      renderFrame();
    } catch (e) { log(`[WARN] Failed to setup audio visualizers: ${e}`); }
  }

  function toggleMute() {
    isMuted = !isMuted;
    if (localStream) localStream.getAudioTracks().forEach(t => t.enabled = !isMuted);
  }

  function toggleVideo() {
    isVideoOff = !isVideoOff;
    if (localStream) localStream.getVideoTracks().forEach(t => t.enabled = !isVideoOff);
  }

  async function leaveRoom() {
    if (visualizerFrameId) cancelAnimationFrame(visualizerFrameId);
    if (audioCtx) audioCtx.close();
    try { await invoke('close_call'); } catch (e) {}
    window.location.reload();
  }

  async function connect() {
    connectionState = 'CONNECTING';
    try {
      if (!audioCtx) audioCtx = new window.AudioContext({ sampleRate: 48000 });
      if (audioCtx.state === 'suspended') audioCtx.resume();
      
      const { listen } = await import('@tauri-apps/api/event');
      await listen('webrtc-connecting', (e) => log(`[RUST] Connecting: ${e.payload}`));
      await listen('webrtc-connected', (e) => {
        connectionState = 'CONNECTED';
        log(`[RUST] Connected: ${e.payload}`);
      });

      // --- WEBCODECS DECODING SETUP ---
      const remoteCanvas = document.createElement('canvas');
      remoteCanvas.width = 640; remoteCanvas.height = 480;
      const remoteCtx = remoteCanvas.getContext('2d');
      const remoteCanvasStream = remoteCanvas.captureStream(30);
      
      function tryBindRemoteVideo() {
        if (remoteVideoRef) {
          remoteVideoRef.srcObject = remoteCanvasStream;
          remoteVideoRef.play().catch(() => {});
          if (localStream) setupVisualizers(localStream, remoteVideoRef);
        } else { requestAnimationFrame(tryBindRemoteVideo); }
      }
      tryBindRemoteVideo();

      let videoDecoder: any = null;
      let remoteFrameCount = 0;
      let waitingForKeyframe = true;
      let videoTimestamp = 0;
      let currentDescBuf: ArrayBuffer | null = null;

      function createVideoDecoder() {
        waitingForKeyframe = true;
        currentDescBuf = null;
        videoDecoder = new (window as any).VideoDecoder({
          output: (frame: any) => {
            remoteFrameCount++;
            if (remoteFrameCount === 1) log(`[DECODER] ✓ First frame decoded (${frame.displayWidth}x${frame.displayHeight})`);
            if (remoteCtx) remoteCtx.drawImage(frame, 0, 0, remoteCanvas.width, remoteCanvas.height);
            frame.close();
          },
          error: (e: any) => {
            log(`[DECODER-ERROR] ${e.name}: ${e.message}`);
            setTimeout(() => createVideoDecoder(), 100);
          }
        });
      }
      createVideoDecoder();

      let audioPlaybackTime = 0;
      let decodedAudioCount = 0;
      const audioDecoder = new (window as any).AudioDecoder({
        output: (audioData: any) => {
          if (!audioCtx) return;
          decodedAudioCount++;
          const buffer = audioCtx.createBuffer(1, audioData.numberOfFrames, audioData.sampleRate);
          const channelData = new Float32Array(audioData.numberOfFrames);
          audioData.copyTo(channelData, { planeIndex: 0 });
          buffer.copyToChannel(channelData, 0);
          const source = audioCtx.createBufferSource();
          source.buffer = buffer;
          if (remoteAnalyser) source.connect(remoteAnalyser);
          else source.connect(audioCtx.destination);
          const now = audioCtx.currentTime;
          const startAt = Math.max(now, audioPlaybackTime);
          source.start(startAt);
          audioPlaybackTime = startAt + buffer.duration;
          audioData.close();
        },
        error: (e: any) => log(`[ERR] AudioDecoder: ${e}`)
      });
      audioDecoder.configure({ codec: 'opus', sampleRate: 48000, numberOfChannels: 1 });

      await listen('webrtc-media-data', (event) => {
        const [kind, data] = event.payload as [string, number[]];
        const uint8 = new Uint8Array(data);
        if (uint8.length === 0) return;

        if (kind === 'video') {
          const isIDR = h264.containsIDR(uint8);
          if (waitingForKeyframe) {
            if (!isIDR) return;
            waitingForKeyframe = false;
            const newDesc = h264.createAVCCDescriptionFromAnnexB(uint8);
            if (newDesc) {
              currentDescBuf = newDesc;
              videoDecoder.configure({
                codec: 'avc1.42001f',
                hardwareAcceleration: 'prefer-hardware',
                description: currentDescBuf
              });
            } else { waitingForKeyframe = true; return; }
          }
          if (videoDecoder && videoDecoder.state === 'configured') {
            videoTimestamp += 33333;
            const avccData = h264.isAnnexB(uint8) ? h264.annexBToAVCC(uint8) : uint8;
            videoDecoder.decode(new (window as any).EncodedVideoChunk({
              type: isIDR ? 'key' : 'delta',
              timestamp: videoTimestamp,
              data: avccData
            }));
          }
        } else if (kind === 'audio') {
          audioDecoder.decode(new (window as any).EncodedAudioChunk({
            type: 'key',
            timestamp: performance.now() * 1000,
            data: uint8
          }));
        }
      });

      // --- CAPTURE & ENCODE ---
      localStream = await navigator.mediaDevices.getUserMedia({ video: true, audio: true });
      if (localVideoRef) localVideoRef.srcObject = localStream;

      const TARGET_FPS = 15;
      const FRAME_INTERVAL_MS = 1000 / TARGET_FPS;
      const VIDEO_WIDTH = 320; const VIDEO_HEIGHT = 240; const VIDEO_BITRATE = 250_000;
      const localCanvas = document.createElement('canvas');
      localCanvas.width = VIDEO_WIDTH; localCanvas.height = VIDEO_HEIGHT;
      const localCtx = localCanvas.getContext('2d');

      const videoEncoder = new (window as any).VideoEncoder({
        output: (chunk: any) => {
          if (connectionState === 'CONNECTED') {
            const raw = new Uint8Array(chunk.byteLength);
            chunk.copyTo(raw);
            const annexBData = h264.isAnnexB(raw) ? raw : h264.avccToAnnexB(raw);
            invoke('send_video_chunk', { data: Array.from(annexBData), timestamp: chunk.timestamp });
          }
        },
        error: (e: any) => log(`[ERR] VideoEncoder: ${e.message}`)
      });
      videoEncoder.configure({ codec: 'avc1.42001f', width: VIDEO_WIDTH, height: VIDEO_HEIGHT, bitrate: VIDEO_BITRATE, latencyMode: 'realtime' });

      let lastEncodeTime = performance.now();
      let lastKeyframeTime = performance.now();
      function encodeVideo() {
        const now = performance.now();
        if (now - lastEncodeTime >= FRAME_INTERVAL_MS) {
          lastEncodeTime = now;
          if (localVideoRef && localVideoRef.readyState >= 2 && localCtx) {
            localCtx.drawImage(localVideoRef, 0, 0, VIDEO_WIDTH, VIDEO_HEIGHT);
            const shouldKeyframe = (now - lastKeyframeTime) > 2000;
            if (shouldKeyframe) lastKeyframeTime = now;
            const frame = new (window as any).VideoFrame(localCanvas, { timestamp: now * 1000 });
            videoEncoder.encode(frame, { keyFrame: shouldKeyframe });
            frame.close();
          }
        }
        requestAnimationFrame(encodeVideo);
      }
      encodeVideo();

      const audioSource = audioCtx.createMediaStreamSource(localStream);
      const scriptNode = audioCtx.createScriptProcessor(2048, 1, 1);
      const audioEncoder = new (window as any).AudioEncoder({
        output: (chunk: any) => {
          if (connectionState === 'CONNECTED') {
            const data = new Uint8Array(chunk.byteLength);
            chunk.copyTo(data);
            invoke('send_audio_chunk', { data: Array.from(data), timestamp: chunk.timestamp });
          }
        },
        error: (e: any) => log(`[ERR] AudioEncoder: ${e}`)
      });
      audioEncoder.configure({ codec: 'opus', sampleRate: 48000, numberOfChannels: 1, bitrate: 64000 });

      let audioTime = 0;
      scriptNode.onaudioprocess = (e) => {
        if (connectionState !== 'CONNECTED') return;
        const pcm = e.inputBuffer.getChannelData(0);
        const audioData = new (window as any).AudioData({
          format: 'f32-planar', sampleRate: 48000, numberOfFrames: pcm.length,
          numberOfChannels: 1, timestamp: audioTime, data: pcm
        });
        audioTime += (pcm.length / 48000) * 1000000;
        audioEncoder.encode(audioData);
        audioData.close();
      };
      audioSource.connect(scriptNode);
      scriptNode.connect(audioCtx.destination);

      await invoke('start_quick_call', { roomId, wsUrl: sigServer });
    } catch (e) { log(`[ERR] ${e}`); connectionState = 'DISCONNECTED'; }
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

<div
  class="container"
  class:fullscreen-mode={isFullscreen}
  class:target-local={fullscreenTarget === 'local'}
  class:target-remote={fullscreenTarget === 'remote'}
  onmousemove={handleMouseMove}
  role="main"
>
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
        <VideoBox 
          title="LOCAL_TX [H.264]" 
          bind:videoRef={localVideoRef} 
          audioLevel={localAudioLevel} 
          isFullscreen={isFullscreen} 
          isTarget={fullscreenTarget === 'local'}
          showExitButton={showExitButton}
          onToggleFullscreen={() => toggleFullscreen('local')}
          class="local"
        />
        <VideoBox 
          title="REMOTE_RX [H.264]" 
          bind:videoRef={remoteVideoRef} 
          audioLevel={remoteAudioLevel} 
          isFullscreen={isFullscreen} 
          isTarget={fullscreenTarget === 'remote'}
          showExitButton={showExitButton}
          onToggleFullscreen={() => toggleFullscreen('remote')}
          class="remote"
        />
      </div>
      <div class="media-controls mt-4">
        <button onclick={toggleMute}>[{isMuted ? 'UNMUTE_MIC' : 'MUTE_MIC'}]</button>
        <button onclick={toggleVideo}>[{isVideoOff ? 'ENABLE_VIDEO' : 'DISABLE_VIDEO'}]</button>
        <button class="danger" onclick={leaveRoom}>[ ABORT_UPLINK ]</button>
      </div>
    {/if}

    <Terminal {logs} />
  </div>
</div>
