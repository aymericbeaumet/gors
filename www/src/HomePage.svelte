<script lang="ts">
import { onDestroy } from "svelte";
import type { NavigateTo } from "./navigation";
import "./styles/home.css";

const BREW_INSTALL_COMMAND = "brew install aymericbeaumet/tap/gors";

export let active: boolean;
export let navigateTo: NavigateTo;

let installCommandCopied = false;
let installCommandTimer: ReturnType<typeof setTimeout> | null = null;

async function copyInstallCommand() {
	try {
		await navigator.clipboard.writeText(BREW_INSTALL_COMMAND);
		installCommandCopied = true;
		if (installCommandTimer) clearTimeout(installCommandTimer);
		installCommandTimer = setTimeout(() => {
			installCommandCopied = false;
		}, 2000);
	} catch {
		/* ignore */
	}
}

onDestroy(() => {
	if (installCommandTimer) clearTimeout(installCommandTimer);
});
</script>

{#if active}
  <div class="home-route">
    <section class="hero">
      <div class="hero-copy">
        <h1>gors</h1>
        <p class="hero-subtitle">
          gors compiles real Go source to readable Rust through a verified semantic pipeline with explicit representation choices.
        </p>
        <div class="hero-actions">
          <button class="install-command" class:copied={installCommandCopied} type="button" title="Copy install command" on:click={copyInstallCommand}>
            <code>{BREW_INSTALL_COMMAND}</code>
            <span class="install-copy" aria-hidden="true">{installCommandCopied ? "Copied" : "Copy"}</span>
          </button>
          <a href="/playground" class="primary-link" on:click={(event) => navigateTo("playground", event)}>Try in Playground</a>
        </div>
      </div>

      <div class="compiler-card" aria-label="Go to Rust compiler pipeline preview">
        <div class="compiler-card-heading">
          <h2>From Go semantics to readable Rust.</h2>
          <p>Each verified phase turns Go semantics into an explicit, inspectable compiler product.</p>
        </div>

        <ol class="compiler-pipeline" aria-label="Compiler pipeline">
          <li class="pipeline-stage frontend-stage">
            <div class="pipeline-stage-label">
              <span class="pipeline-stage-number" aria-hidden="true">01</span>
              <p class="pipeline-stage-kind">Frontend</p>
            </div>
            <div class="pipeline-stage-copy">
              <h3>Parse Go source</h3>
              <p>Scanner and parser build an ephemeral Go AST.</p>
            </div>
          </li>
          <li class="pipeline-stage semantic-stage">
            <div class="pipeline-stage-label">
              <span class="pipeline-stage-number" aria-hidden="true">02</span>
              <p class="pipeline-stage-kind">Semantics</p>
            </div>
            <div class="pipeline-stage-copy">
              <h3>Type and resolve</h3>
              <p>Names and exact Go types become typed HIR.</p>
            </div>
          </li>
          <li class="pipeline-stage mir-stage">
            <div class="pipeline-stage-label">
              <span class="pipeline-stage-number" aria-hidden="true">03</span>
              <p class="pipeline-stage-kind">Go MIR</p>
            </div>
            <div class="pipeline-stage-copy">
              <h3>Order and verify</h3>
              <p>Evaluation order and control flow become verified Go MIR.</p>
            </div>
          </li>
          <li class="pipeline-stage backend-stage">
            <div class="pipeline-stage-label">
              <span class="pipeline-stage-number" aria-hidden="true">04</span>
              <p class="pipeline-stage-kind">Rust backend</p>
            </div>
            <div class="pipeline-stage-copy">
              <h3>Represent and emit</h3>
              <p>Explicit representation choices become verified Rust IR, then formatted Rust source.</p>
            </div>
          </li>
        </ol>
      </div>
    </section>

    <section class="home-details" aria-label="gors benefits">
      <article>
        <h3>Try the pipeline quickly</h3>
        <p>The <a href="/playground" on:click={(event) => navigateTo("playground", event)}>playground</a> is a convenient way to inspect generated Rust for small programs.</p>
      </article>
      <article>
        <h3>Shared compiler path</h3>
        <p>The playground and CLI share the same verified compiler pipeline and source-map model.</p>
      </article>
      <article>
        <h3>Pinned SDK inputs</h3>
        <p>The resolver selects pinned SDK source inputs, which compile through the same semantic pipeline as user packages.</p>
      </article>
      <article>
        <h3>Measured compatibility</h3>
        <p>Every recorded result comes from generated programs compared with the repository-pinned Go toolchain, while unsupported cases stay visible. <a href="/conformance" on:click={(event) => navigateTo("conformance", event)}>Explore the results.</a></p>
      </article>
      <article>
        <h3>Generic stdlib compilation</h3>
        <p>Standard-library packages compile from their Go source through the same typed semantics and verified IR pipeline as user programs.</p>
      </article>
      <article>
        <h3>Hermetic comparisons</h3>
        <p>The test harness uses the repository-pinned Go SDK instead of whatever Go version happens to be installed locally.</p>
      </article>
    </section>
  </div>
{/if}
