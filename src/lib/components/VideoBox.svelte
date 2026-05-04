<script lang="ts">
  interface Props {
    title: string;
    videoRef: HTMLVideoElement | null;
    audioLevel: number;
    isFullscreen: boolean;
    isTarget: boolean;
    showExitButton: boolean;
    onToggleFullscreen: () => void;
    class?: string;
  }
  let { 
    title, 
    videoRef = $bindable(), 
    audioLevel, 
    isFullscreen, 
    isTarget,
    showExitButton,
    onToggleFullscreen, 
    class: className 
  }: Props = $props();

  function getAsciiBar(level: number) {
    const maxBars = 10;
    const filled = Math.min(maxBars, Math.max(0, Math.floor((level / 255) * maxBars * 1.5)));
    return '[' + '|'.repeat(filled) + '.'.repeat(maxBars - filled) + ']';
  }
</script>

<div class="video-box {className}" class:is-fullscreen={isTarget}>
  {#if !isTarget}
    <span>> {title} {getAsciiBar(audioLevel)}</span>
  {/if}
  
  <video bind:this={videoRef} autoplay muted={title.includes('LOCAL')} playsinline class:glow={audioLevel > 20}></video>
  
  {#if isTarget}
    {#if showExitButton}
      <button class="exit-fullscreen-btn icon-btn" title="Exit Fullscreen" onclick={onToggleFullscreen}>
        <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M8 3v3a2 2 0 0 1-2 2H3"/><path d="M21 8h-3a2 2 0 0 1-2-2V3"/><path d="M3 16h3a2 2 0 0 1 2 2v3"/><path d="M16 21v-3a2 2 0 0 1 2-2h3"/></svg>
      </button>
    {/if}
  {:else if !isFullscreen}
    <button class="fullscreen-btn icon-btn" title="Fullscreen" onclick={onToggleFullscreen}>
      <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M8 3H5a2 2 0 0 0-2 2v3"/><path d="M21 8V5a2 2 0 0 0-2-2h-3"/><path d="M3 16v3a2 2 0 0 0 2 2h3"/><path d="M16 21h3a2 2 0 0 0 2-2v-3"/></svg>
    </button>
  {/if}
</div>

<style>
  .video-box {
    border: 1px dashed var(--border-color);
    flex: 1;
    display: flex;
    flex-direction: column;
    padding: 0.5rem;
    position: relative;
  }

  .video-box.is-fullscreen {
    border: none;
    padding: 0;
    width: 100%;
    height: 100%;
    background: black;
    display: flex;
    align-items: center;
    justify-content: center;
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

  .is-fullscreen video {
    width: 100%;
    height: 100%;
    object-fit: contain;
    background: black;
  }

  video.glow {
    box-shadow: 0 0 15px rgba(0, 255, 65, 0.2);
  }

  .fullscreen-btn, .exit-fullscreen-btn {
    position: absolute;
    background: transparent;
    border: 1px solid var(--primary-glow);
    color: var(--primary-glow);
    cursor: pointer;
    transition: all 0.2s;
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10;
  }

  .fullscreen-btn {
    bottom: 0.5rem;
    right: 0.5rem;
    width: 32px;
    height: 32px;
    opacity: 0.7;
  }

  .exit-fullscreen-btn {
    bottom: 1rem;
    right: 1rem;
    width: 48px;
    height: 48px;
    animation: slideIn 0.3s ease-out;
  }

  .fullscreen-btn:hover, .exit-fullscreen-btn:hover {
    background: var(--primary-glow);
    color: var(--bg-color);
    box-shadow: 0 0 8px var(--primary-glow);
    opacity: 1;
  }

  @keyframes slideIn {
    from { opacity: 0; transform: translate(10px, 10px); }
    to { opacity: 1; transform: translate(0, 0); }
  }
</style>
