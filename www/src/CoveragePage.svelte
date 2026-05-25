<script lang="ts">
import { onDestroy, onMount } from "svelte";
import {
	gostdlibCoverage,
	gostdlibCoverageSummary,
	type GostdlibCoveragePackage,
	type GostdlibCoverageSymbol,
} from "./gostdlib-coverage";

const FIXTURE_GITHUB_BASE =
	"https://github.com/aymericbeaumet/gors/tree/master/tests/fixtures/go_programs";

type CoverageStatusFilter = "all" | "green" | "yellow" | "red";

const STATUS_FILTERS: {
	readonly value: CoverageStatusFilter;
	readonly label: string;
	readonly className: string;
}[] = [
	{ value: "all", label: "All", className: "all" },
	{ value: "green", label: "Green", className: "tested" },
	{ value: "yellow", label: "Yellow", className: "partial" },
	{ value: "red", label: "Red", className: "none" },
];

function normalizeStatusFilter(value: string | null): CoverageStatusFilter {
	if (value === "green" || value === "yellow" || value === "red") return value;
	return "all";
}

function readUrlFilters(): {
	readonly query: string;
	readonly status: CoverageStatusFilter;
} {
	const params = new URLSearchParams(window.location.search);
	return {
		query: params.get("q") ?? "",
		status: normalizeStatusFilter(params.get("color")),
	};
}

const initialFilters = readUrlFilters();
let stdlibFilter = initialFilters.query;
let appliedStdlibFilter = initialFilters.query;
let statusFilter: CoverageStatusFilter = initialFilters.status;
let filteredGostdlibCoverage: readonly GostdlibCoveragePackage[] =
	gostdlibCoverage;
let searchWorker: Worker | null = null;
let nextSearchRequestId = 1;
let activeSearchRequestId = 0;
let debounceTimer: ReturnType<typeof setTimeout> | undefined;
let mounted = false;

function symbolCoverageTitle(symbol: GostdlibCoverageSymbol): string {
	if (!symbol.tested) return `${symbol.kind}; not tested`;
	return `${symbol.kind}; tested by ${symbol.fixtures.join(", ")}`;
}

function packageCoverageClass(item: GostdlibCoveragePackage): string {
	if (item.testedSymbolCount === 0) return "none";
	if (item.testedSymbolCount === item.symbolCount) return "tested";
	return "partial";
}

function packageCoverageColor(
	item: GostdlibCoveragePackage,
): CoverageStatusFilter {
	const coverageClass = packageCoverageClass(item);
	if (coverageClass === "tested") return "green";
	if (coverageClass === "partial") return "yellow";
	return "red";
}

function fixtureGithubUrl(fixture: string): string {
	return `${FIXTURE_GITHUB_BASE}/${fixture}`;
}

function syncFiltersToUrl(query: string, status: CoverageStatusFilter): void {
	const url = new URL(window.location.href);
	const trimmedQuery = query.trim();
	if (trimmedQuery) url.searchParams.set("q", trimmedQuery);
	else url.searchParams.delete("q");
	if (status === "all") url.searchParams.delete("color");
	else url.searchParams.set("color", status);
	const next = `${url.pathname}${url.search}${url.hash}`;
	const current = `${window.location.pathname}${window.location.search}${window.location.hash}`;
	if (next !== current) window.history.replaceState({}, "", next);
}

function coveragePercent(tested: number, total: number): string {
	if (total === 0) return "0%";
	return `${((tested / total) * 100).toFixed(1)}%`;
}

function coverageMetric(tested: number, total: number): string {
	return `${tested}/${total} (${coveragePercent(tested, total)})`;
}

function applyPackageIndexes(packageIndexes: readonly number[]): void {
	filteredGostdlibCoverage = packageIndexes.map(
		(index) => gostdlibCoverage[index],
	);
}

function runMainThreadSearch(
	query: string,
	status: CoverageStatusFilter,
): void {
	const terms = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
	filteredGostdlibCoverage = gostdlibCoverage.filter((item) => {
		if (status !== "all" && packageCoverageColor(item) !== status) return false;
		if (terms.length === 0) return true;
		const searchText = [
			item.packagePath,
			...item.fixtures,
			...item.symbols.flatMap((symbol) => [
				symbol.name,
				symbol.kind,
				symbol.tested ? "tested" : "not tested untested",
				...symbol.fixtures,
			]),
		]
			.join(" ")
			.toLowerCase();
		return terms.every((term) => searchText.includes(term));
	});
}

function requestCoverageSearch(
	query: string,
	status: CoverageStatusFilter,
): void {
	syncFiltersToUrl(query, status);
	const id = nextSearchRequestId++;
	activeSearchRequestId = id;
	if (!searchWorker) {
		runMainThreadSearch(query, status);
		return;
	}
	searchWorker.postMessage({ type: "search", id, query, status });
}

function debounceCoverageSearch(): void {
	if (debounceTimer) clearTimeout(debounceTimer);
	debounceTimer = setTimeout(() => {
		appliedStdlibFilter = stdlibFilter;
	}, 120);
}

$: {
	if (mounted) {
		stdlibFilter;
		debounceCoverageSearch();
	}
}
$: {
	if (mounted) {
		statusFilter;
		requestCoverageSearch(appliedStdlibFilter, statusFilter);
	}
}

onMount(() => {
	searchWorker = new Worker(
		new URL("./coverage-search-worker.ts", import.meta.url),
		{
			type: "module",
		},
	);
	searchWorker.onmessage = ({
		data,
	}: MessageEvent<{ id: number; packageIndexes: readonly number[] }>) => {
		if (data.id !== activeSearchRequestId) return;
		applyPackageIndexes(data.packageIndexes);
	};
	searchWorker.onerror = () => {
		searchWorker?.terminate();
		searchWorker = null;
		runMainThreadSearch(appliedStdlibFilter, statusFilter);
	};
	mounted = true;
	requestCoverageSearch(stdlibFilter, statusFilter);
});

onDestroy(() => {
	if (debounceTimer) clearTimeout(debounceTimer);
	searchWorker?.terminate();
});
</script>

<section class="coverage-page">
  <div class="conformance-section spec-conformance">
    <h1>Go specification conformance</h1>
    <p>TODO</p>
  </div>

  <div class="conformance-section">
    <div class="coverage-intro">
      <div>
        <h1>Go standard library conformance</h1>
        <p>
          Runnable fixtures are compared against the pinned Go SDK, then the tested selectors are matched against
          the embedded stdlib symbol list used by the compiler.
        </p>
      </div>
    </div>

    <div class="report-summary">
      <div class="report-metric">
        <strong>{coverageMetric(gostdlibCoverageSummary.testedPackageCount, gostdlibCoverageSummary.packageCount)}</strong>
        <span>packages tested</span>
      </div>
      <div class="report-metric">
        <strong>{coverageMetric(gostdlibCoverageSummary.testedSymbolCount, gostdlibCoverageSummary.symbolCount)}</strong>
        <span>symbols tested</span>
      </div>
      <label class="report-filter">
        <span>Filter</span>
        <input bind:value={stdlibFilter} type="search" placeholder="package, function, fixture, tested" autocomplete="off" />
      </label>
      <div class="status-filter" role="group" aria-label="Coverage status filter">
        {#each STATUS_FILTERS as filter}
          <button
            type="button"
            class={filter.className}
            class:active={statusFilter === filter.value}
            on:click={() => (statusFilter = filter.value)}
            aria-pressed={statusFilter === filter.value}
          >
            {filter.label}
          </button>
        {/each}
      </div>
    </div>

    <div class="report-list" role="table" aria-label="Go stdlib integration coverage">
      <div class="report-list-head" role="row">
        <span role="columnheader">Package</span>
        <span role="columnheader">Functions / symbols</span>
        <span role="columnheader">Fixtures</span>
      </div>
      <div class="report-list-body">
        {#each filteredGostdlibCoverage as item}
          <div class="coverage-row" role="row">
            <div class="package-cell" role="cell">
              <code class={packageCoverageClass(item)}>{item.packagePath}</code>
              <span class={packageCoverageClass(item)}>{item.testedSymbolCount}/{item.symbolCount} tested</span>
            </div>
            <div class="symbol-cell" role="cell">
              {#each item.symbols as symbol}
                <span class="symbol-token" class:tested={symbol.tested} class:untested={!symbol.tested} title={symbolCoverageTitle(symbol)}>
                  <span>{symbol.name}</span>
                  <small>{symbol.kind}</small>
                </span>
              {/each}
            </div>
            <div class="fixture-cell" role="cell">
              {#each item.fixtures as fixture}
                <a href={fixtureGithubUrl(fixture)} target="_blank" rel="noopener">
                  <code>{fixture}</code>
                </a>
              {/each}
            </div>
          </div>
        {:else}
          <div class="report-empty">No matching coverage</div>
        {/each}
      </div>
    </div>
  </div>
</section>

<style>
  .coverage-page {
    display: flex;
    flex: 1;
    max-width: 100%;
    min-height: 0;
    flex-direction: column;
    gap: 16px;
    overflow-x: clip;
    padding: 24px;
    background: #f5f7fb;
    color: #1f2328;
  }

  .conformance-section {
    display: flex;
    min-width: 0;
    flex-direction: column;
    gap: 16px;
    flex-shrink: 0;
  }

  .conformance-section:last-child {
    flex: 1;
    min-height: 0;
  }

  .spec-conformance {
    padding-bottom: 16px;
    border-bottom: 1px solid #d0d7de;
  }

  .spec-conformance p {
    margin: 0;
    color: #57606a;
    font-size: 13px;
  }

  .coverage-intro {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 24px;
    flex-shrink: 0;
  }

  h1 {
    margin: 0;
    color: #1f2328;
    font-size: 32px;
    line-height: 1.05;
  }

  .coverage-intro p:last-child {
    max-width: 740px;
    margin: 10px 0 0;
    color: #57606a;
    font-size: 15px;
    line-height: 1.45;
  }

  .report-summary {
    display: grid;
    grid-template-columns:
      repeat(2, minmax(170px, 210px)) minmax(220px, 1fr)
      minmax(220px, auto);
    gap: 12px;
    flex-shrink: 0;
    min-width: 0;
  }

  .report-metric,
  .report-filter,
  .status-filter {
    min-height: 58px;
    border: 1px solid #d0d7de;
    border-radius: 8px;
    background: #ffffff;
  }

  .report-metric {
    display: flex;
    flex-direction: column;
    justify-content: center;
    padding: 8px 12px;
  }

  .report-metric strong {
    color: #1f2328;
    font-size: 18px;
    font-weight: 650;
    line-height: 1;
    white-space: nowrap;
  }

  .report-metric span,
  .report-filter span {
    margin-top: 4px;
    color: #57606a;
    font-size: 12px;
  }

  .report-filter {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
  }

  .report-filter span {
    margin: 0;
    flex-shrink: 0;
  }

  .report-filter input {
    width: 100%;
    min-width: 0;
    height: 34px;
    padding: 0 10px;
    border: 1px solid #d0d7de;
    border-radius: 4px;
    background: #ffffff;
    color: #1f2328;
    font: inherit;
    font-size: 13px;
  }

  .report-filter input:focus {
    outline: none;
    border-color: #0969da;
  }

  .status-filter {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 6px;
    padding: 10px;
  }

  .status-filter button {
    min-width: 0;
    border: 1px solid #d0d7de;
    border-radius: 4px;
    background: #ffffff;
    color: #57606a;
    cursor: pointer;
    font: inherit;
    font-size: 12px;
    font-weight: 650;
  }

  .status-filter button:hover,
  .status-filter button.active {
    color: #1f2328;
  }

  .status-filter button.tested:hover,
  .status-filter button.tested.active {
    border-color: #2da44e;
    background: #dafbe1;
    color: #1a7f37;
  }

  .status-filter button.partial:hover,
  .status-filter button.partial.active {
    border-color: #d4a72c;
    background: #fff8c5;
    color: #9a6700;
  }

  .status-filter button.none:hover,
  .status-filter button.none.active {
    border-color: #cf222e;
    background: #ffebe9;
    color: #cf222e;
  }

  .report-list {
    display: flex;
    flex: 1;
    max-width: 100%;
    min-height: 0;
    min-width: 0;
    flex-direction: column;
    overflow: hidden;
    border: 1px solid #d0d7de;
    border-radius: 8px;
    background: #ffffff;
  }

  .report-list-head,
  .coverage-row {
    display: grid;
    grid-template-columns: minmax(150px, 0.72fr) minmax(0, 2.1fr) minmax(150px, 0.9fr);
    align-items: start;
    column-gap: 16px;
    min-width: 0;
  }

  .report-list-head {
    padding: 10px 14px;
    border-bottom: 1px solid #d0d7de;
    background: #f6f8fa;
    color: #57606a;
    font-size: 12px;
    font-weight: 650;
  }

  .report-list-body {
    flex: 1;
    min-height: 0;
    min-width: 0;
    overflow-x: hidden;
    overflow-y: auto;
  }

  .coverage-row {
    padding: 12px 14px;
    border-bottom: 1px solid #d8dee4;
  }

  .coverage-row:last-child {
    border-bottom: 0;
  }

  .package-cell {
    display: flex;
    min-width: 0;
    flex-direction: column;
    gap: 6px;
  }

  .package-cell code {
    font-family: "Fira Code Variable", "Fira Code", monospace;
    font-size: 13px;
    font-weight: 700;
    word-break: break-word;
  }

  .package-cell code.none,
  .package-cell span.none {
    color: #cf222e;
  }

  .package-cell code.tested,
  .package-cell span.tested {
    color: #1a7f37;
  }

  .package-cell code.partial,
  .package-cell span.partial {
    color: #9a6700;
  }

  .package-cell span {
    color: #57606a;
    font-size: 12px;
  }

  .symbol-cell,
  .fixture-cell {
    display: flex;
    max-width: 100%;
    min-width: 0;
    flex-wrap: wrap;
    gap: 6px;
  }

  .fixture-cell a {
    max-width: 100%;
    text-decoration: none;
  }

  .fixture-cell a:hover code {
    border-color: #8250df;
    background: #f0e7ff;
  }

  .symbol-token,
  .fixture-cell code {
    max-width: 100%;
    padding: 3px 7px;
    overflow-wrap: anywhere;
    border: 1px solid #d0d7de;
    border-radius: 4px;
    background: #f6f8fa;
    color: #1f2328;
    font-family: "Fira Code Variable", "Fira Code", monospace;
    font-size: 12px;
    line-height: 1.35;
  }

  .symbol-token {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }

  .symbol-token span {
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .symbol-token.tested {
    border-color: #2da44e;
    background: #dafbe1;
  }

  .symbol-token.untested {
    border-color: #d0d7de;
    color: #57606a;
  }

  .symbol-token small {
    color: inherit;
    font-size: 10px;
    line-height: 1;
    opacity: 0.7;
  }

  .fixture-cell code {
    color: #8250df;
  }

  .report-empty {
    padding: 28px 14px;
    color: #57606a;
    font-size: 13px;
  }

  @media (max-width: 900px) {
    .coverage-page {
      padding: 16px;
    }

    .coverage-intro {
      align-items: flex-start;
      flex-direction: column;
    }

    .report-summary {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }

    .report-summary,
    .coverage-intro {
      min-width: 0;
    }

    .report-filter,
    .status-filter {
      grid-column: 1 / -1;
    }

    .report-list-head {
      display: none;
    }

    .coverage-row {
      grid-template-columns: 1fr;
      gap: 10px;
    }
  }
</style>
