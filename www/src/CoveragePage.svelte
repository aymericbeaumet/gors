<script lang="ts">
const implemented = [
	"Typed, name-resolved HIR with explicit IDs and source spans",
	"Explicit control-flow MIR with Go assignment ordering",
	"Mandatory conservative Rust representation lowering into verified Rust IR",
	"Terminal syn emitter with no semantic repair passes",
	"Primitive values, free functions, scalar control flow, print and println",
] as const;

const backlog = [
	"Packages, imports, initialization, and generic stdlib compilation",
	"Declared and aggregate types, methods, interfaces, and generics",
	"Pointers, slices, maps, function values, closures, and exact ABI lowering",
	"Range, switch, defer, panic/recover, goroutines, channels, and select",
	"Incremental semantic queries and deterministic parallel compilation",
] as const;
</script>

<section class="coverage-page">
	<header>
		<p class="eyebrow">Hard cutover</p>
		<h1>Conformance baseline reset</h1>
		<p class="summary">
			The previous compiler and its historical pass counts were deleted. The
			current compiler accepts a deliberately narrow bootstrap subset and returns
			structured diagnostics for everything else. No old backend or fallback is
			used to inflate these numbers.
		</p>
		<a
			href="https://github.com/aymericbeaumet/gors/blob/master/COMPILER_AUDIT.md"
			target="_blank"
			rel="noopener"
		>
			Read the architecture audit and roadmap
		</a>
	</header>

	<div class="status-grid">
		<article>
			<h2>Implemented foundation</h2>
			<ul>
				{#each implemented as item}
					<li>{item}</li>
				{/each}
			</ul>
		</article>

		<article>
			<h2>Migration backlog</h2>
			<ul>
				{#each backlog as item}
					<li>{item}</li>
				{/each}
			</ul>
		</article>
	</div>

	<p class="policy">
		Go specification, stdlib, repository, and arbitrary-program fixtures remain
		in the repository as the differential migration inventory. A new percentage
		will be published only after a complete, unfiltered run of the new backend.
	</p>
</section>

<style>
	.coverage-page {
		display: grid;
		align-content: start;
		gap: 24px;
		width: min(1100px, 100%);
		margin: 0 auto;
		padding: 48px 24px 72px;
		color: #1f2328;
	}

	header {
		display: grid;
		gap: 12px;
	}

	.eyebrow {
		margin: 0;
		color: #0969da;
		font-weight: 700;
		letter-spacing: 0.08em;
		text-transform: uppercase;
	}

	h1,
	h2,
	p {
		margin: 0;
	}

	h1 {
		font-size: clamp(32px, 5vw, 56px);
		line-height: 1;
	}

	.summary,
	.policy {
		max-width: 820px;
		color: #57606a;
		font-size: 17px;
		line-height: 1.6;
	}

	a {
		width: fit-content;
		color: #0969da;
		font-weight: 600;
	}

	.status-grid {
		display: grid;
		grid-template-columns: repeat(2, minmax(0, 1fr));
		gap: 16px;
	}

	article {
		padding: 24px;
		border: 1px solid #d0d7de;
		border-radius: 12px;
		background: #fff;
	}

	h2 {
		font-size: 20px;
	}

	ul {
		display: grid;
		gap: 10px;
		margin: 18px 0 0;
		padding-left: 20px;
		color: #57606a;
		line-height: 1.45;
	}

	.policy {
		padding: 18px 20px;
		border-left: 4px solid #bf8700;
		background: #fff8c5;
	}

	@media (max-width: 760px) {
		.status-grid {
			grid-template-columns: 1fr;
		}

		.coverage-page {
			padding: 32px 16px 48px;
		}
	}
</style>
