<script lang="ts">
import "./styles/vm-terminal.css";

export let visible: boolean;
export let started: boolean;
export let title: string;
export let terminalElement: HTMLDivElement;
export let close: () => void;

function onOverlayClick(event: MouseEvent) {
	if (event.target === event.currentTarget) close();
}

function onKeydown(event: KeyboardEvent) {
	if (event.key === "Escape" && visible) close();
}
</script>

<svelte:window on:keydown={onKeydown} />

<!-- svelte-ignore a11y-click-events-have-key-events -->
<!-- svelte-ignore a11y-no-static-element-interactions -->
<div class="vm-terminal-overlay" class:visible on:click={onOverlayClick}>
  <div class="vm-terminal-panel">
    <div class="vm-terminal-header">
      <div class="vm-terminal-left"></div>
      <span class="vm-terminal-title">Linux VM</span>
      <div class="vm-terminal-right">
        <button class="vm-terminal-close" title="Close" on:click={close}>&times;</button>
      </div>
    </div>
    {#if !started}
      <div class="vm-spinner-container">
        <div class="vm-spinner"></div>
        <span class="vm-spinner-label">{title}</span>
      </div>
    {/if}
    <div class="vm-terminal-body" bind:this={terminalElement} style:display={started ? "" : "none"}></div>
  </div>
</div>
