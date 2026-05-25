<script lang="ts">
import {
	gostdlibCoverage,
	gostdlibCoverageSummary,
	type GostdlibCoveragePackage,
	type GostdlibCoverageSymbol,
} from "./gostdlib-coverage";

const FIXTURE_GITHUB_BASE =
	"https://github.com/aymericbeaumet/gors/tree/master/tests/fixtures/go_programs";

let stdlibFilter = "";

function coverageMatchesFilter(
	item: GostdlibCoveragePackage,
	query: string,
): boolean {
	if (!query) return true;
	const packageStatus = item.tested ? "tested" : "not tested untested";
	return (
		item.packagePath.toLowerCase().includes(query) ||
		packageStatus.includes(query) ||
		item.fixtures.some((fixture) => fixture.toLowerCase().includes(query)) ||
		item.symbols.some(
			(symbol) =>
				symbol.name.toLowerCase().includes(query) ||
				symbol.kind.includes(query) ||
				(symbol.tested ? "tested" : "not tested untested").includes(query) ||
				symbol.fixtures.some((fixture) =>
					fixture.toLowerCase().includes(query),
				),
		)
	);
}

function symbolCoverageTitle(symbol: GostdlibCoverageSymbol): string {
	if (!symbol.tested) return `${symbol.kind}; not tested`;
	return `${symbol.kind}; tested by ${symbol.fixtures.join(", ")}`;
}

function packageCoverageClass(item: GostdlibCoveragePackage): string {
	if (item.testedSymbolCount === 0) return "none";
	if (item.testedSymbolCount === item.symbolCount) return "tested";
	return "partial";
}

function fixtureGithubUrl(fixture: string): string {
	return `${FIXTURE_GITHUB_BASE}/${fixture}`;
}

$: stdlibQuery = stdlibFilter.trim().toLowerCase();
$: filteredGostdlibCoverage = gostdlibCoverage.filter((item) =>
	coverageMatchesFilter(item, stdlibQuery),
);
$: visibleStdlibSymbolCount = filteredGostdlibCoverage.reduce(
	(total, item) => total + item.symbols.length,
	0,
);
$: visibleStdlibTestedSymbolCount = filteredGostdlibCoverage.reduce(
	(total, item) => total + item.testedSymbolCount,
	0,
);
$: visibleStdlibUntestedSymbolCount =
	visibleStdlibSymbolCount - visibleStdlibTestedSymbolCount;
</script>

<section class="coverage-page">
  <div class="coverage-intro">
    <div>
      <p class="eyebrow">Integration coverage</p>
      <h1>Go stdlib coverage</h1>
      <p>
        Runnable fixtures are compared against the pinned Go SDK, then the tested selectors are matched against
        the embedded stdlib symbol list used by the compiler.
      </p>
    </div>
  </div>

  <div class="report-summary">
    <div class="report-metric">
      <strong>{gostdlibCoverageSummary.fixtureCount}</strong>
      <span>fixtures</span>
    </div>
    <div class="report-metric">
      <strong>{gostdlibCoverageSummary.testedPackageCount}/{gostdlibCoverageSummary.packageCount}</strong>
      <span>packages tested</span>
    </div>
    <div class="report-metric">
      <strong>{gostdlibCoverageSummary.testedSymbolCount}/{gostdlibCoverageSummary.symbolCount}</strong>
      <span>symbols tested</span>
    </div>
    <div class="report-metric">
      <strong>{visibleStdlibUntestedSymbolCount}</strong>
      <span>visible untested</span>
    </div>
    <label class="report-filter">
      <span>Filter</span>
      <input bind:value={stdlibFilter} type="search" placeholder="package, function, fixture, tested" autocomplete="off" />
    </label>
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

  .coverage-intro {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 24px;
    flex-shrink: 0;
  }

  .eyebrow {
    margin: 0 0 6px;
    color: #57606a;
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0;
    text-transform: uppercase;
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
    grid-template-columns: repeat(4, minmax(110px, 170px)) minmax(220px, 1fr);
    gap: 12px;
    flex-shrink: 0;
    min-width: 0;
  }

  .report-metric,
  .report-filter {
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
    font-size: 22px;
    font-weight: 650;
    line-height: 1;
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

    .report-filter {
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
