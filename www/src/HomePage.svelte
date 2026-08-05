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
        <p class="eyebrow">Go compiler frontend, Rust backend</p>
        <h1>gors</h1>
        <p class="hero-subtitle">
          gors is a Go-to-Rust compiler pipeline: it parses real Go source, builds verified semantic IRs, chooses explicit Rust representations, and prints normal Rust code.
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
        <h2>Backed by a powerful compiler.</h2>
        <div class="pipeline-flow" aria-hidden="true">
          <div class="flow-node go-node"><span>Go source</span></div>
          <span class="flow-arrow"></span>
          <div class="flow-node"><span>Scanner</span></div>
          <span class="flow-arrow"></span>
          <div class="flow-node"><span>Parser</span></div>
          <span class="flow-arrow"></span>
          <div class="flow-node go-ast-node"><span>Go AST</span></div>
          <span class="flow-arrow"></span>
          <div class="flow-node"><span>Typed HIR</span></div>
          <span class="flow-arrow"></span>
          <div class="flow-node rust-ast-node"><span>Go MIR</span></div>
          <span class="flow-arrow"></span>
          <div class="flow-node"><span>Rust lowering</span></div>
          <span class="flow-arrow"></span>
          <div class="flow-node"><span>Rust IR</span></div>
          <span class="flow-arrow"></span>
          <div class="flow-node rust-node"><span>Rust source</span></div>
          <i class="flow-pulse"></i>
        </div>
      </div>
    </section>

    <section class="home-details" aria-label="gors benefits">
      <article>
        <h3>Try the pipeline quickly</h3>
        <p>The <a href="/playground" on:click={(event) => navigateTo("playground", event)}>playground</a> is a convenient way to inspect generated Rust for small programs.</p>
      </article>
      <article>
        <h3>Shared compiler path</h3>
        <p>Typed HIR, explicit-order Go MIR, mandatory Rust representation lowering, verified Rust IR, terminal emission, and source-map lookup use the same path as the CLI.</p>
      </article>
      <article>
        <h3>Pinned SDK inputs</h3>
        <p>The resolver exposes build-selected SDK source metadata only. Package and stdlib lowering return after their semantics exist in HIR and MIR.</p>
      </article>
      <article>
        <h3>Measured compatibility</h3>
        <p>Executable fixtures compare generated programs with the pinned Go toolchain, and unsupported features return structured diagnostics. <a href="/conformance" on:click={(event) => navigateTo("conformance", event)}>Explore the results.</a></p>
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
