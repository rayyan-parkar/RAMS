<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';

  let roomId = $state('FYP-DEMO-2026');
  let sigServer = $state('ws://127.0.0.1:8080');
  let connectionState = $state('DISCONNECTED');
  let logs: string[] = $state(['System Initialised... Ready.']);

  function log(msg: string) {
    logs = [...logs, msg];
  }

  async function connect() {
    connectionState = 'CONNECTING';
    log(`[SYS] establishing link to ${sigServer}...`);
    log(`[SYS] authenticating room key: ${roomId}`);
    
    try {
      // In a subsequent iteration, this will invoke the real Rust RamsBuilder 
      // await invoke('start_rtc', { room: roomId, sigUrl: sigServer });
      
      setTimeout(() => {
        connectionState = 'CONNECTED';
        log('[OK] ICE Gathering Complete.');
        log('[OK] WebRTC P2P Channel LIVE.');
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
          <div class="placeholder glow">CAMERA ACTIVE</div>
        </div>
        <div class="video-box remote">
          <span>> REMOTE_RX [VP8]</span>
          <div class="placeholder">AWAITING FRAMES...</div>
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
  
  .placeholder {
    height: 250px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 0.9rem;
    color: var(--text-muted);
    background: #050505;
  }
  
  .placeholder.glow {
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
