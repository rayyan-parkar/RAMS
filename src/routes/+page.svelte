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
  let remoteAnalyser: AnalyserNode | null = null;
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

      remoteAnalyser = audioCtx.createAnalyser();
      remoteAnalyser.fftSize = 256;
      // Note: We no longer capture from rVideo because it's a canvas stream with no audio.
      // Instead, the AudioDecoder will connect directly to this remoteAnalyser.
      remoteAnalyser.connect(audioCtx.destination); 


      const localDataArray = new Uint8Array(localAnalyser.frequencyBinCount);
      const remoteDataArray = new Uint8Array(remoteAnalyser.frequencyBinCount);

      function renderFrame() {
        localAnalyser.getByteFrequencyData(localDataArray);
        if (remoteAnalyser) {
          remoteAnalyser.getByteFrequencyData(remoteDataArray);
        }
        
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
  }

  function toggleVideo() {
    isVideoOff = !isVideoOff;
    if (localStream) {
      localStream.getVideoTracks().forEach(t => t.enabled = !isVideoOff);
    }
  }

  function auditEnvironment() {
    if (typeof (window as any).VideoDecoder === 'undefined') {
      log('[DECODER] ERROR: VideoDecoder not supported!');
    } else {
      log('[DECODER] VideoDecoder available.');
    }
  }

  async function leaveRoom() {
    if (visualizerFrameId) cancelAnimationFrame(visualizerFrameId);
    if (audioCtx) audioCtx.close();
    try {
      await invoke('close_call');
    } catch (e) {
      // silent
    }
    window.location.reload();
  }

  async function connect() {
    connectionState = 'CONNECTING';
    
    try {
      if (!audioCtx) {
        audioCtx = new window.AudioContext({ sampleRate: 48000 });
      }
      if (audioCtx.state === 'suspended') {
        audioCtx.resume().catch(() => {});
      }
      
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
          remoteVideoRef.play().catch(() => {});
          // Remote video element ready
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

      // ---- H.264 UTILITIES ----
      const NAL_TYPE_NAMES: Record<number, string> = {
        1: 'NON_IDR_SLICE', 5: 'IDR_SLICE', 6: 'SEI',
        7: 'SPS', 8: 'PPS', 9: 'AUD', 0: 'UNSPECIFIED'
      };

      /** Return the NAL unit type (5 LSBs of first byte after start code) */
      function nalType(b: number): number { return b & 0x1f; }
      function nalName(t: number): string { return NAL_TYPE_NAMES[t] ?? `UNKNOWN(${t})`; }

      /** Parse SPS and PPS from an AVCDecoderConfigurationRecord (the 'description' field). */
      function parseSPSPPS(desc: ArrayBuffer): { sps: Uint8Array[], pps: Uint8Array[] } {
        const d = new Uint8Array(desc);
        const sps: Uint8Array[] = [];
        const pps: Uint8Array[] = [];
        if (d.length < 7) { return { sps, pps }; }
        // AVCDecoderConfigurationRecord layout:
        // byte 0: configurationVersion (always 1)
        // byte 5 lower 5 bits: numSPS
        let offset = 5;
        const numSPS = d[offset] & 0x1f; offset++;
        for (let i = 0; i < numSPS; i++) {
          const len = (d[offset] << 8) | d[offset + 1]; offset += 2;
          sps.push(d.slice(offset, offset + len)); offset += len;
        }
        const numPPS = d[offset]; offset++;
        for (let i = 0; i < numPPS; i++) {
          const len = (d[offset] << 8) | d[offset + 1]; offset += 2;
          pps.push(d.slice(offset, offset + len)); offset += len;
        }

        return { sps, pps };
      }

      /** Convert AVCC (length-prefixed NALUs) to Annex-B (start-code-prefixed).
       *  Optionally prepend SPS/PPS before the first NALU. */
      function avccToAnnexB(avcc: Uint8Array, prependNals?: Uint8Array[]): Uint8Array {
        const startCode = new Uint8Array([0, 0, 0, 1]);
        const parts: Uint8Array[] = [];

        // Prepend SPS/PPS if provided
        if (prependNals) {
          for (const nal of prependNals) {
            parts.push(startCode);
            parts.push(nal);
          }
        }

        // Walk the AVCC buffer: [4-byte length][NALU data] ...
        let pos = 0;
        let naluCount = 0;
        while (pos + 4 <= avcc.length) {
          const naluLen = (avcc[pos] << 24) | (avcc[pos+1] << 16) | (avcc[pos+2] << 8) | avcc[pos+3];
          pos += 4;
          if (naluLen <= 0 || pos + naluLen > avcc.length) {

            break;
          }
          const nt = nalType(avcc[pos]);
          if (naluCount < 5 || nt === 5 || nt === 7 || nt === 8) {
            // Log first few NALUs and all keyframe/SPS/PPS
          }
          naluCount++;
          parts.push(startCode);
          parts.push(avcc.slice(pos, pos + naluLen));
          pos += naluLen;
        }

        // Concatenate all parts
        const totalLen = parts.reduce((sum, p) => sum + p.length, 0);
        const result = new Uint8Array(totalLen);
        let offset = 0;
        for (const p of parts) { result.set(p, offset); offset += p.length; }
        return result;
      }

      /** Detect if a buffer starts with Annex-B start codes */
      function isAnnexB(data: Uint8Array): boolean {
        return (data.length >= 4 && data[0] === 0 && data[1] === 0 && data[2] === 0 && data[3] === 1) ||
               (data.length >= 3 && data[0] === 0 && data[1] === 0 && data[2] === 1);
      }

      /** Split Annex-B stream into individual NAL units (without start codes) */
      function parseAnnexB(data: Uint8Array): Uint8Array[] {
        const nals: Uint8Array[] = [];
        let i = 0;
        while (i < data.length) {
          let start = -1;
          for (let j = i; j < data.length - 2; j++) {
            if (data[j] === 0 && data[j+1] === 0 && data[j+2] === 1) {
              const is4Byte = (j > 0 && data[j-1] === 0);
              const scStart = is4Byte ? j - 1 : j;
              const scLen = is4Byte ? 4 : 3;
              if (i < scStart) nals.push(data.subarray(i, scStart));
              start = scStart + scLen;
              break;
            }
          }
          if (start === -1) {
            if (i < data.length) nals.push(data.subarray(i));
            break;
          }
          i = start;
        }
        return nals;
      }

      /** Convert Annex-B to AVCC (length-prefixed NALUs) */
      function annexBToAVCC(data: Uint8Array): Uint8Array {
        const nals = parseAnnexB(data);
        const totalLen = nals.reduce((sum, n) => sum + 4 + n.length, 0);
        const avcc = new Uint8Array(totalLen);
        let offset = 0;
        for (const n of nals) {
          avcc[offset]   = (n.length >> 24) & 0xff;
          avcc[offset+1] = (n.length >> 16) & 0xff;
          avcc[offset+2] = (n.length >> 8) & 0xff;
          avcc[offset+3] = n.length & 0xff;
          avcc.set(n, offset + 4);
          offset += 4 + n.length;
        }
        return avcc;
      }

      /** Extract SPS/PPS from Annex-B stream and construct AVCDecoderConfigurationRecord */
      function createAVCCDescriptionFromAnnexB(annexB: Uint8Array): ArrayBuffer | null {
        const nals = parseAnnexB(annexB);
        let sps: Uint8Array | null = null;
        let pps: Uint8Array | null = null;
        for (const nal of nals) {
          const t = nalType(nal[0]);
          if (t === 7 && !sps) sps = nal;
          if (t === 8 && !pps) pps = nal;
        }
        if (!sps || !pps) return null;

        const desc = new Uint8Array(5 + 3 + sps.length + 3 + pps.length);
        desc[0] = 1; // configurationVersion
        desc[1] = sps[1]; // AVCProfileIndication
        desc[2] = sps[2]; // profile_compatibility
        desc[3] = sps[3]; // AVCLevelIndication
        desc[4] = 0xff; // lengthSizeMinusOne = 3 (so 4 bytes)
        desc[5] = 0xe1; // numOfSequenceParameterSets = 1
        desc[6] = (sps.length >> 8) & 0xff;
        desc[7] = sps.length & 0xff;
        desc.set(sps, 8);
        
        const offset = 8 + sps.length;
        desc[offset] = 1; // numOfPictureParameterSets = 1
        desc[offset+1] = (pps.length >> 8) & 0xff;
        desc[offset+2] = pps.length & 0xff;
        desc.set(pps, offset + 3);
        
        return desc.buffer;
      }

      /** List NAL unit types in an Annex-B stream */
      function listAnnexBNalTypes(data: Uint8Array): string[] {
        const types: string[] = [];
        for (let i = 0; i < data.length - 4; i++) {
          if (data[i] === 0 && data[i+1] === 0 && data[i+2] === 0 && data[i+3] === 1) {
            types.push(nalName(nalType(data[i+4])));
          }
        }
        return types;
      }

      /** Detect if an Annex-B stream contains an IDR (keyframe) */
      function containsIDR(data: Uint8Array): boolean {
        for (let i = 0; i < data.length - 4; i++) {
          if (data[i] === 0 && data[i+1] === 0 && data[i+2] === 0 && data[i+3] === 1) {
            if (nalType(data[i+4]) === 5) return true;
          }
        }
        return false;
      }

      // Receiver decoder state
      let currentDescBuf: ArrayBuffer | null = null;

      function createVideoDecoder() {
        waitingForKeyframe = true;
        currentDescBuf = null;
        videoDecoder = new (window as any).VideoDecoder({
          output: (frame: any) => {
            remoteFrameCount++;
            if (remoteFrameCount === 1) {
              log(`[DECODER] ✓ First frame decoded (${frame.displayWidth}x${frame.displayHeight})`);
            } else if (remoteFrameCount % 500 === 0) {
              log(`[DECODER] Frame #${remoteFrameCount}`);
            }
            if (remoteCtx) {
              remoteCtx.drawImage(frame, 0, 0, remoteCanvas.width, remoteCanvas.height);
            }
            frame.close();
          },
          error: (e: any) => {
            decoderCrashCount++;
            log(`[DECODER-ERROR] Crash #${decoderCrashCount}: ${e.name}: ${e.message}`);
            if (decoderCrashCount < 20) {
              setTimeout(() => createVideoDecoder(), 100);
            } else if (decoderCrashCount === 20) {
              log('[DECODER-ERROR] FATAL: Decoder crashed 20 times. Giving up.');
            }
          }
        });
        log('[DECODER] Instance created, waiting for keyframe...');
      }
      createVideoDecoder();

      // === SELF-TEST: Verify H.264 encode→decode roundtrip ===
      // Tests AVCC+description (native path) and Annex-B (streaming path)
      try {
        const testCanvas = document.createElement('canvas');
        testCanvas.width = 160; // use reasonable size, 64x64 can hit encoder edge cases
        testCanvas.height = 120;
        const tctx = testCanvas.getContext('2d')!;
        tctx.fillStyle = 'blue';
        tctx.fillRect(0, 0, 160, 120);
        tctx.fillStyle = 'white';
        tctx.font = '16px sans-serif';
        tctx.fillText('SELF-TEST', 10, 60);

        const testFrame = new (window as any).VideoFrame(testCanvas, { timestamp: 0 });

        // We'll try multiple decode strategies in sequence
        const strategies = [
          { name: 'AVCC+desc (software)', accel: 'prefer-software', useAVCC: true },
          { name: 'AVCC+desc (hardware)', accel: 'prefer-hardware', useAVCC: true },
          { name: 'Annex-B (software)', accel: 'prefer-software', useAVCC: false },
          { name: 'Annex-B (hardware)', accel: 'prefer-hardware', useAVCC: false },
        ];

        // Collect encoded chunks and the decoder configuration metadata first
        let encodedChunks: { type: string, timestamp: number, data: Uint8Array, desc?: ArrayBuffer }[] = [];

        const testEncoder = new (window as any).VideoEncoder({
          output: (chunk: any, metadata: any) => {
            const raw = new Uint8Array(chunk.byteLength);
            chunk.copyTo(raw);
            const desc = metadata?.decoderConfig?.description;
            encodedChunks.push({
              type: chunk.type,
              timestamp: chunk.timestamp,
              data: raw,
              desc: desc ? (desc instanceof ArrayBuffer ? desc : (desc.buffer || desc)) : undefined
            });
          },
          error: (e: any) => {}
        });
        testEncoder.configure({ codec: 'avc1.42001f', width: 160, height: 120, bitrate: 500_000, latencyMode: 'realtime', hardwareAcceleration: 'prefer-hardware' });
        testEncoder.encode(testFrame, { keyFrame: true });
        testFrame.close();
        await testEncoder.flush();
        testEncoder.close();


        // Now try each decode strategy
        for (const strat of strategies) {
          try {
            let decoded = false;
            const dec = new (window as any).VideoDecoder({
              output: (frame: any) => { decoded = true; frame.close(); },
              error: (_e: any) => {}
            });

            for (const chunk of encodedChunks) {
              let configObj: any = { codec: 'avc1.42001f', hardwareAcceleration: strat.accel };
              let feedData: Uint8Array;

              if (chunk.desc) {
                configObj.description = chunk.desc;
              }

              if (strat.useAVCC) {
                feedData = chunk.data;
              } else {
                feedData = isAnnexB(chunk.data) ? chunk.data : avccToAnnexB(chunk.data);
              }

              dec.configure(configObj);
              dec.decode(new (window as any).EncodedVideoChunk({
                type: chunk.type,
                timestamp: chunk.timestamp,
                data: feedData
              }));
            }

            await dec.flush();
            dec.close();
          } catch (e: any) {
            log(`[SELF-TEST] ${strat.name}: ✗ FAIL (${e.name}: ${e.message})`);
          }
        }
      } catch (e: any) {
        log(`[SELF-TEST] Exception: ${e.name}: ${e.message}`);
      }

      // (AudioContext was initialized at the top of connect())
      audioCtx?.resume();

      let audioPlaybackTime = 0;
      let decodedAudioCount = 0;
      const audioDecoder = new (window as any).AudioDecoder({
        output: (audioData: any) => {
          if (!audioCtx) return;
          
          if (audioCtx.state === 'suspended') {
            audioCtx.resume();
          }

          decodedAudioCount++;
          if (decodedAudioCount === 1) {
            log('[DECODER] Audio decode: first frame received');
          }

          const buffer = audioCtx.createBuffer(1, audioData.numberOfFrames, audioData.sampleRate);
          const channelData = new Float32Array(audioData.numberOfFrames);
          audioData.copyTo(channelData, { planeIndex: 0 });
          buffer.copyToChannel(channelData, 0);

          const source = audioCtx.createBufferSource();
          source.buffer = buffer;
          
          // Connect to destination AND the visualizer analyser
          source.connect(audioCtx.destination);
          if (remoteAnalyser) {
            source.connect(remoteAnalyser);
          }

          // Schedule playback sequentially to avoid overlapping/gaps
          const now = audioCtx.currentTime;
          const startAt = Math.max(now, audioPlaybackTime);
          source.start(startAt);
          audioPlaybackTime = startAt + buffer.duration;
          audioData.close();
        },
        error: (e: any) => log(`[ERR] AudioDecoder: ${e}`)
      });
      try {
        audioDecoder.configure({ codec: 'opus', sampleRate: 48000, numberOfChannels: 1 });
        log(`[OK] AudioDecoder configured, state=${audioDecoder.state}`);
      } catch (e) {
        log(`[ERR] AudioDecoder config failed: ${e}`);
      }

      let rxVideoChunks = 0;
      let rxAudioChunks = 0;

      // Check if H.264 is actually supported in this browser engine
      try {
        const support = await (window as any).VideoDecoder.isConfigSupported({ codec: 'avc1.42001f' });
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
          const isIDR = containsIDR(uint8);
          
          if (waitingForKeyframe) {
            if (!isIDR) {
              if (rxVideoChunks === 1) log('[DECODER] Waiting for keyframe...');
              return;
            }
            waitingForKeyframe = false;
            log(`[DECODER] ✓ Keyframe received (frame #${rxVideoChunks}, ${uint8.length} bytes)`);
            
            // WebKitGTK WebCodecs STRICTLY requires AVCC description to configure the decoder!
            // We must extract SPS and PPS from this Annex-B IDR keyframe to build the description.
            const newDesc = createAVCCDescriptionFromAnnexB(uint8);
            if (newDesc) {
              currentDescBuf = newDesc;
              log(`[DECODER] Description extracted (${currentDescBuf.byteLength}B)`);
              
              videoDecoder.configure({
                codec: 'avc1.42001f',
                hardwareAcceleration: 'prefer-hardware',
                description: currentDescBuf
              });
              log('[DECODER] Configured with hardware acceleration enabled');
            } else {
              log(`[ERR] Failed to extract SPS/PPS from IDR frame! Cannot configure decoder.`);
              waitingForKeyframe = true; // Wait for the next one
              return;
            }
          }

          try {
            if (videoDecoder && videoDecoder.state === 'configured') {
              videoTimestamp += 33333;
              
              // WebKitGTK WebCodecs STRICTLY requires AVCC formatted chunks (length prefixed)!
              // We must convert the incoming Annex-B stream back to AVCC before feeding it.
              const avccData = isAnnexB(uint8) ? annexBToAVCC(uint8) : uint8;

              videoDecoder.decode(new (window as any).EncodedVideoChunk({
                type: isIDR ? 'key' : 'delta',
                timestamp: videoTimestamp,
                data: avccData
              }));
            }
          } catch (e: any) {
            log(`[DECODER-ERROR] Decode failed: ${e.name}: ${e.message}`);
          }
        } else if (kind === 'audio') {
          rxAudioChunks++;
          try {
            // Use a sequence-based timestamp to ensure they are unique and monotonic
            // even if multiple packets arrive in the same tick.
            // 20ms = 20000us per Opus packet.
            const syntheticTimestamp = rxAudioChunks * 20000;
            
            audioDecoder.decode(new (window as any).EncodedAudioChunk({
              type: 'key',
              timestamp: syntheticTimestamp,
              data: uint8
            }));
          } catch (e) {
            // audio decode error - silent
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
      // PERFORMANCE: Optimize for low-spec hardware (15 FPS, lower resolution, aggressive throttling)
      const TARGET_FPS = 15;
      const FRAME_INTERVAL_MS = 1000 / TARGET_FPS; // ~66ms per frame
      const VIDEO_WIDTH = 320;   // Reduced from 640
      const VIDEO_HEIGHT = 240;  // Reduced from 480
      const VIDEO_BITRATE = 250_000; // Reduced from 1M
      
      localCanvas.width = VIDEO_WIDTH;
      localCanvas.height = VIDEO_HEIGHT;
      const localCtx = localCanvas.getContext('2d');
      
      // Frame throttling state
      let lastEncodeTime = performance.now();
      let pendingFrameEncode = false;
      let pendingFrameTimestamp = 0;
      
      // Cached SPS/PPS NALUs extracted from encoder metadata (for prepending to keyframes)
      let encoderSPS: Uint8Array[] = [];
      let encoderPPS: Uint8Array[] = [];
      let txFrameCount = 0;

      const videoEncoder = new (window as any).VideoEncoder({
        output: (chunk: any, metadata: any) => {
          if (connectionState === 'CONNECTED') {
            const raw = new Uint8Array(chunk.byteLength);
            chunk.copyTo(raw);

            // Extract SPS/PPS from metadata when available (usually on first keyframe)
            const desc = metadata?.decoderConfig?.description;
            if (desc) {
              const descBuf = desc instanceof ArrayBuffer ? desc : (desc.buffer || desc);
              const parsed = parseSPSPPS(descBuf);
              encoderSPS = parsed.sps;
              encoderPPS = parsed.pps;
              log(`[TX] Captured SPS/PPS from encoder metadata`);
            }

            // Convert AVCC → Annex-B (don't prepend SPS/PPS - encoder includes them inline for keyframes)
            const gotAnnexB = isAnnexB(raw);
            let annexBData: Uint8Array;
            if (gotAnnexB) {
              annexBData = raw;
            } else {
              annexBData = avccToAnnexB(raw);
            }

            txFrameCount++;
            if (txFrameCount <= 3 || txFrameCount % 300 === 0) {
              const nalTypes = listAnnexBNalTypes(annexBData);
              log(`[TX #${txFrameCount}] ${chunk.type} frame, ${raw.length}→${annexBData.length} bytes (${gotAnnexB ? 'annexb' : 'avcc→annexb'}), NALs=[${nalTypes.join(', ')}]`);
            }

            invoke('send_video_chunk', { 
              data: Array.from(annexBData),
              timestamp: chunk.timestamp 
            });
          }
        },
        error: (e: any) => log(`[ERR] VideoEncoder: name=${e.name}, message="${e.message}"`)
      });
      try {
        videoEncoder.configure({ 
          codec: 'avc1.42001f', // Constrained Baseline Profile, Level 3.1
          width: VIDEO_WIDTH, 
          height: VIDEO_HEIGHT, 
          bitrate: VIDEO_BITRATE,
          latencyMode: 'realtime',
          hardwareAcceleration: 'prefer-software' // Software encoding is often more stable on low-spec
        });
        log(`[OK] VideoEncoder configured: ${VIDEO_WIDTH}x${VIDEO_HEIGHT} @ ${TARGET_FPS}fps, ${VIDEO_BITRATE/1000}kbps`);
      } catch (e) {
        log(`[ERR] VideoEncoder config failed: ${e}`);
      }

      let frameCount = 0;
      let lastKeyframeTime = performance.now();
      const KEYFRAME_INTERVAL_MS = 2000; // Keyframe every 2 seconds at 15 FPS = every ~30 frames
      
      function encodeVideo() {
        const now = performance.now();
        
        // Throttle to TARGET_FPS
        if (now - lastEncodeTime < FRAME_INTERVAL_MS) {
          requestAnimationFrame(encodeVideo);
          return;
        }
        
        lastEncodeTime = now;
        
        if (localVideoRef && localVideoRef.readyState >= 2 && localCtx) {
          localCtx.drawImage(localVideoRef, 0, 0, localCanvas.width, localCanvas.height);
          
          // Emit keyframe periodically (every 2s)
          const shouldKeyframe = (now - lastKeyframeTime) > KEYFRAME_INTERVAL_MS;
          if (shouldKeyframe) {
            lastKeyframeTime = now;
          }
          
          try {
            const frame = new (window as any).VideoFrame(localCanvas, { timestamp: now * 1000 });
            videoEncoder.encode(frame, { keyFrame: shouldKeyframe });
            frame.close();
            frameCount++;
            
            if (frameCount % 30 === 0) {
              log(`[TX-VIDEO] Encoded frame #${frameCount} (throttled to ${TARGET_FPS}fps)`);
            }
          } catch (e) {
            log(`[ERR] VideoFrame encode failed: ${e}`);
          }
        }
        
        requestAnimationFrame(encodeVideo);
      }
      encodeVideo(); // Start grabbing frames

      // Audio Encoding: Reuse playback AudioContext to reduce system load on low-spec hardware
      // Note: We reuse audioCtx which was already created for playback visualizers
      if (!audioCtx) {
        audioCtx = new window.AudioContext({ sampleRate: 48000 });
      }
      const audioSource = audioCtx.createMediaStreamSource(stream);
      // Using ScriptProcessorNode (deprecated but widely supported) to grab PCM data
      // Smaller buffer size (2048) for lower latency instead of default 4096
      const scriptNode = audioCtx.createScriptProcessor(2048, 1, 1);
      
      const audioEncoder = new (window as any).AudioEncoder({
        output: (chunk: any) => {
          if (connectionState === 'CONNECTED') {
            const data = new Uint8Array(chunk.byteLength);
            chunk.copyTo(data);
            if (Math.random() < 0.01) log(`[TX] Sending ${data.length} byte audio frame`);
            invoke('send_audio_chunk', { 
              data: Array.from(data),
              timestamp: chunk.timestamp
            });
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
      let audioSendCount = 0;
      let audioFrameSkip = 0;
      scriptNode.onaudioprocess = (e) => {
        if (connectionState !== 'CONNECTED') return;
        const pcm = e.inputBuffer.getChannelData(0);
        
        if (audioSendCount < 3) {
          log(`[TX-AUDIO] Capturing PCM, length=${pcm.length}`);
          audioSendCount++;
        }

        try {
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
        } catch (e) {
          // Backpressure: skip frames if encoder is overloaded
          audioFrameSkip++;
          if (audioFrameSkip % 10 === 0) {
            log(`[WARN] Audio encode backpressure (skipped ${audioFrameSkip} frames)`);
          }
        }
      };
      audioSource.connect(scriptNode);
      scriptNode.connect(audioCtx.destination);
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
          <span>> LOCAL_TX [H.264]</span>
          <video bind:this={localVideoRef} autoplay muted playsinline class="glow"></video>
          <div class="mic-visualizer">MIC: {getAsciiBar(localAudioLevel)}</div>
        </div>
        <div class="video-box remote">
          <span>> REMOTE_RX [H.264]</span>
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
