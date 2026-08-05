<script lang="ts">
import type { AppRoute, NavigateTo } from "./navigation";
import "./styles/site-header.css";

export let route: AppRoute;
export let navigateTo: NavigateTo;
</script>

<header class="site-header">
  <a class="brand" href="/" on:click={(event) => navigateTo("home", event)}>gors</a>
  <nav class="site-nav" aria-label="Primary navigation">
    <a href="/" class:active={route === "home"} on:click={(event) => navigateTo("home", event)}>Home</a>
    <a href="/conformance" class:active={route === "conformance"} on:click={(event) => navigateTo("conformance", event)}>Conformance</a>
    <a href="/playground" class:active={route === "playground"} on:click={(event) => navigateTo("playground", event)}>Playground</a>
  </nav>
  <div class="spacer"></div>
  <a class="github-link" href="https://github.com/aymericbeaumet/gors" target="_blank" rel="noopener" aria-label="GitHub repository">
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M12 .5a12 12 0 0 0-3.79 23.39c.6.11.82-.26.82-.58v-2.03c-3.34.73-4.04-1.61-4.04-1.61-.55-1.39-1.34-1.76-1.34-1.76-1.09-.75.08-.73.08-.73 1.21.08 1.85 1.24 1.85 1.24 1.07 1.84 2.81 1.31 3.5 1 .11-.78.42-1.31.76-1.61-2.67-.3-5.47-1.33-5.47-5.93 0-1.31.47-2.38 1.24-3.22-.12-.3-.54-1.53.12-3.18 0 0 1.01-.32 3.3 1.23a11.5 11.5 0 0 1 6.01 0c2.29-1.55 3.3-1.23 3.3-1.23.66 1.65.24 2.88.12 3.18.77.84 1.24 1.91 1.24 3.22 0 4.61-2.81 5.63-5.48 5.93.43.37.81 1.1.81 2.22v3.29c0 .32.22.7.82.58A12 12 0 0 0 12 .5Z"/>
    </svg>
    <span>GitHub</span>
  </a>
</header>
