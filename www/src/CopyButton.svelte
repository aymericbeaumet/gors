<script>
  export let getContent;
  export let title = 'Copy';

  let copied = false;
  let timer;

  async function copy() {
    const content = getContent();
    if (!content) return;
    try {
      await navigator.clipboard.writeText(content);
      copied = true;
      clearTimeout(timer);
      timer = setTimeout(() => { copied = false; }, 2000);
    } catch { /* ignore */ }
  }
</script>

<button class="copy-button" class:copied {title} on:click={copy}>
  <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
    <path d="M16 1H4c-1.1 0-2 .9-2 2v14h2V3h12V1zm3 4H8c-1.1 0-2 .9-2 2v14c0 1.1.9 2 2 2h11c1.1 0 2-.9 2-2V7c0-1.1-.9-2-2-2zm0 16H8V7h11v14z"/>
  </svg>
  <span>{copied ? 'Copied!' : 'Copy'}</span>
</button>

<style>
  .copy-button {
    padding: 4px 8px;
    background: transparent;
    border: 1px solid #30363d;
    border-radius: 4px;
    color: #8b949e;
    font-size: 11px;
    font-family: inherit;
    cursor: pointer;
    display: flex;
    align-items: center;
    gap: 4px;
    transition: all 0.15s ease;
  }
  .copy-button:hover { background: #21262d; border-color: #8b949e; color: #c9d1d9; }
  .copy-button.copied { background: #238636; border-color: #238636; color: #ffffff; }
  .copy-button svg { width: 12px; height: 12px; fill: currentColor; }
</style>
