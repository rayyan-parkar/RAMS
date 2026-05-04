<script lang="ts">
  interface Props {
    logs: string[];
  }
  let { logs }: Props = $props();
</script>

<div class="terminal mt-4">
  {#each logs as log}
    <div>{log}</div>
  {/each}
  <div class="blink">_</div>
</div>

<style>
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
    50% {
      opacity: 0;
    }
  }
</style>
