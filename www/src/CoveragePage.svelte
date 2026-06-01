<script lang="ts">
import goSpecReport from "../../gors/tests/reports/go-spec-conformance.json";
import goStdlibReport from "../../gors/tests/reports/go-stdlib-conformance.json";

const FIXTURE_GITHUB_BASE =
	"https://github.com/aymericbeaumet/gors/tree/master/gors/tests/fixtures";

type ConformanceStatus = "passing" | "unsupported";

interface ConformanceSummary {
	readonly groupCount: number;
	readonly passingGroupCount: number;
	readonly caseCount: number;
	readonly passingCaseCount: number;
	readonly unsupportedCaseCount: number;
	readonly fixtureCount: number;
}

interface ConformanceSource {
	readonly title: string;
	readonly url: string;
	readonly languageVersion: string;
	readonly published: string;
	readonly retrieved: string;
}

interface ConformanceCase {
	readonly id: string;
	readonly title: string;
	readonly subtitle: string;
	readonly kind: string;
	readonly status: ConformanceStatus;
	readonly fixtures: readonly string[];
	readonly reason: string;
}

interface ConformanceGroup {
	readonly id: string;
	readonly title: string;
	readonly subtitle: string;
	readonly fixtures: readonly string[];
	readonly summary: ConformanceSummary;
	readonly cases: readonly ConformanceCase[];
}

interface ConformanceReport {
	readonly schemaVersion: number;
	readonly kind: string;
	readonly title: string;
	readonly source: ConformanceSource;
	readonly summary: ConformanceSummary;
	readonly groups: readonly ConformanceGroup[];
}

const specReport = goSpecReport as ConformanceReport;
const stdlibReport = goStdlibReport as ConformanceReport;

const reportViews: readonly {
	readonly report: ConformanceReport;
	readonly fixtureSet: "go_spec" | "go_stdlib";
	readonly groupColumn: string;
	readonly caseColumn: string;
	readonly caseMetricLabel: string;
	readonly sourceLabel: string;
}[] = [
	{
		report: specReport,
		fixtureSet: "go_spec",
		groupColumn: "Category",
		caseColumn: "Tests",
		caseMetricLabel: "tests passing",
		sourceLabel: "Go Language Specification",
	},
	{
		report: stdlibReport,
		fixtureSet: "go_stdlib",
		groupColumn: "Package",
		caseColumn: "Symbols",
		caseMetricLabel: "symbols passing",
		sourceLabel: "Go Standard Library",
	},
];

function symbolCoverageTitle(symbol: ConformanceCase): string {
	if (symbol.status !== "passing") return `${symbol.kind}; not covered`;
	return `${symbol.kind}; tested by ${symbol.fixtures.join(", ")}`;
}

function groupCoverageClass(item: ConformanceGroup): string {
	if (item.summary.passingCaseCount === 0) return "none";
	if (item.summary.passingCaseCount === item.summary.caseCount) return "tested";
	return "partial";
}

function fixtureGithubUrl(
	fixtureSet: "go_spec" | "go_stdlib",
	fixture: string,
): string {
	return `${FIXTURE_GITHUB_BASE}/${fixtureSet}/${fixture}`;
}

function statusLabel(status: ConformanceStatus): string {
	return status === "passing" ? "Passing" : "Uncovered";
}

function statusClass(status: ConformanceStatus): string {
	return status === "passing" ? "tested" : "none";
}

function coveragePercent(tested: number, total: number): string {
	if (total === 0) return "0%";
	return `${((tested / total) * 100).toFixed(1)}%`;
}

function coverageMetric(tested: number, total: number): string {
	return `${tested}/${total} (${coveragePercent(tested, total)})`;
}
</script>

<section class="coverage-page">
  {#each reportViews as view}
    <div class="conformance-section">
      <div class="coverage-intro">
        <div>
          <h1>{view.report.title}</h1>
          <p>
            Generated from the <a href={view.report.source.url} target="_blank" rel="noopener">{view.sourceLabel}</a>
          </p>
        </div>
      </div>

      <div class="report-summary">
        <div class="report-metric">
          <strong>{coverageMetric(view.report.summary.passingCaseCount, view.report.summary.caseCount)}</strong>
          <span>{view.caseMetricLabel}</span>
        </div>
      </div>

      <div class="spec-list" role="table" aria-label={`${view.report.title} report`}>
        <div class="spec-list-head" role="row">
          <span role="columnheader">{view.groupColumn}</span>
          <span role="columnheader">{view.caseColumn}</span>
        </div>
        <div class="spec-list-body">
          {#each view.report.groups as group}
            <div class="spec-category-row" role="row">
              <div class="spec-category-cell package-cell" role="cell">
                <code class={groupCoverageClass(group)}>{group.title}</code>
                <span class={groupCoverageClass(group)}>{group.summary.passingCaseCount}/{group.summary.caseCount} passing</span>
                {#if group.fixtures.length > 0}
                  <div class="fixture-cell">
                    {#each group.fixtures as fixture}
                      <a href={fixtureGithubUrl(view.fixtureSet, fixture)} target="_blank" rel="noopener">
                        <code>{fixture}</code>
                      </a>
                    {/each}
                  </div>
                {/if}
              </div>
              <div class="spec-tests-cell stdlib-symbols-cell" role="cell">
                {#each group.cases as item}
                  <div class={`spec-test stdlib-symbol ${statusClass(item.status)}`} title={symbolCoverageTitle(item)}>
                    <div class="spec-test-main">
                      <strong>{item.title}</strong>
                      <span>{item.subtitle || item.kind}</span>
                      {#if item.reason}
                        <small>{item.reason}</small>
                      {/if}
                      {#if item.fixtures.length > 0}
                        <div class="fixture-cell">
                          {#each item.fixtures as fixture}
                            <a href={fixtureGithubUrl(view.fixtureSet, fixture)} target="_blank" rel="noopener">
                              <code>{fixture}</code>
                            </a>
                          {/each}
                        </div>
                      {/if}
                    </div>
                    <span class={`spec-status ${statusClass(item.status)}`}>
                      {statusLabel(item.status)}
                    </span>
                  </div>
                {/each}
              </div>
            </div>
          {/each}
        </div>
      </div>
    </div>
  {/each}
</section>

<style>
  .coverage-page {
    display: grid;
    flex: 1;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    max-width: 100%;
    height: 100%;
    min-height: 0;
    gap: 12px;
    overflow: hidden;
    padding: 16px;
    background: #f5f7fb;
    color: #1f2328;
  }

  .conformance-section {
    display: flex;
    min-width: 0;
    min-height: 0;
    flex-direction: column;
    gap: 10px;
    overflow-y: auto;
    padding-right: 4px;
  }

  .coverage-intro {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 16px;
    flex-shrink: 0;
  }

  h1 {
    margin: 0;
    color: #1f2328;
    font-size: 24px;
    line-height: 1.05;
  }

  .coverage-intro p:last-child {
    max-width: 740px;
    margin: 6px 0 0;
    color: #57606a;
    font-size: 13px;
    line-height: 1.35;
  }

  .coverage-intro a {
    color: #0969da;
    text-decoration: none;
  }

  .coverage-intro a:hover {
    text-decoration: underline;
  }

  .report-summary {
    display: grid;
    grid-template-columns: minmax(170px, 210px);
    gap: 8px;
    flex-shrink: 0;
    min-width: 0;
  }

  .report-metric {
    min-height: 48px;
    border: 1px solid #d0d7de;
    border-radius: 8px;
    background: #ffffff;
  }

  .report-metric {
    display: flex;
    flex-direction: column;
    justify-content: center;
    padding: 7px 10px;
  }

  .report-metric strong {
    color: #1f2328;
    font-size: 15px;
    font-weight: 650;
    line-height: 1;
    white-space: nowrap;
  }

  .report-metric span {
    margin-top: 3px;
    color: #57606a;
    font-size: 11px;
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
    gap: 4px;
  }

  .package-cell code {
    font-family: "Fira Code Variable", "Fira Code", monospace;
    font-size: 12px;
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
    font-size: 11px;
  }

  .symbol-cell,
  .fixture-cell {
    display: flex;
    max-width: 100%;
    min-width: 0;
    flex-wrap: wrap;
    gap: 4px;
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
    padding: 2px 5px;
    overflow-wrap: anywhere;
    border: 1px solid #d0d7de;
    border-radius: 4px;
    background: #f6f8fa;
    color: #1f2328;
    font-family: "Fira Code Variable", "Fira Code", monospace;
    font-size: 10px;
    line-height: 1.25;
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

  .spec-list {
    display: flex;
    max-width: 100%;
    min-width: 0;
    flex-direction: column;
    overflow: visible;
    border: 1px solid #d0d7de;
    border-radius: 8px;
    background: #ffffff;
  }

  .spec-list-head,
  .spec-category-row {
    display: grid;
    grid-template-columns: minmax(120px, 0.34fr) minmax(0, 1.66fr);
    align-items: start;
    column-gap: 8px;
    min-width: 0;
  }

  .spec-list-head {
    padding: 7px 10px;
    border-bottom: 1px solid #d0d7de;
    background: #f6f8fa;
    color: #57606a;
    font-size: 11px;
    font-weight: 650;
  }

  .spec-list-body {
    min-width: 0;
  }

  .spec-category-row {
    padding: 8px;
    border-bottom: 1px solid #d8dee4;
  }

  .spec-category-row:last-child {
    border-bottom: 0;
  }

  .spec-category-cell {
    display: flex;
    min-width: 0;
    flex-direction: column;
    gap: 4px;
  }

  .spec-category-cell strong {
    color: #1f2328;
    font-size: 12px;
    line-height: 1.25;
  }

  .spec-category-cell span {
    color: #57606a;
    font-size: 11px;
  }

  .spec-tests-cell {
    display: flex;
    min-width: 0;
    flex-direction: column;
    gap: 6px;
  }

  .spec-test {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    gap: 8px;
    min-width: 0;
    padding: 6px;
    border: 1px solid #d0d7de;
    border-left-width: 4px;
    border-radius: 5px;
    background: #ffffff;
  }

  .spec-test.tested {
    border-left-color: #2da44e;
  }

  .spec-test.partial {
    border-left-color: #d4a72c;
  }

  .spec-test.none {
    border-left-color: #cf222e;
  }

  .spec-test-main {
    display: flex;
    min-width: 0;
    flex-direction: column;
    gap: 3px;
  }

  .spec-test-main strong {
    color: #1f2328;
    font-size: 12px;
    line-height: 1.25;
  }

  .spec-test-main span,
  .spec-test-main small {
    color: #57606a;
    font-size: 11px;
    line-height: 1.25;
  }

  .spec-test-main small {
    overflow-wrap: anywhere;
  }

  .spec-status {
    align-self: start;
    padding: 2px 5px;
    border: 1px solid #d0d7de;
    border-radius: 999px;
    font-size: 10px;
    font-weight: 700;
    line-height: 1.2;
    white-space: nowrap;
  }

  .spec-status.tested {
    border-color: #2da44e;
    background: #dafbe1;
    color: #1a7f37;
  }

  .spec-status.partial {
    border-color: #d4a72c;
    background: #fff8c5;
    color: #9a6700;
  }

  .spec-status.none {
    border-color: #cf222e;
    background: #ffebe9;
    color: #cf222e;
  }

  .stdlib-symbols-cell {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    align-items: start;
  }

  .stdlib-symbol {
    min-height: 0;
  }

  .stdlib-symbol .spec-test-main {
    gap: 5px;
  }

  .stdlib-symbol .fixture-cell {
    margin-top: 2px;
  }

  @media (max-width: 900px) {
    .coverage-page {
      display: flex;
      height: auto;
      min-height: 100%;
      overflow: visible;
      padding: 16px;
    }

    .conformance-section {
      overflow: visible;
      padding-right: 0;
    }

    .conformance-section + .conformance-section {
      margin-top: 18px;
    }

    .coverage-intro {
      align-items: flex-start;
      flex-direction: column;
    }

    .report-summary {
      grid-template-columns: minmax(0, 1fr);
    }

    .report-summary,
    .coverage-intro {
      min-width: 0;
    }

    .spec-list-head {
      display: none;
    }

    .spec-category-row,
    .spec-test {
      grid-template-columns: 1fr;
      gap: 10px;
    }

    .stdlib-symbols-cell {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
  }

  @media (max-width: 640px) {
    .stdlib-symbols-cell {
      grid-template-columns: 1fr;
    }
  }
</style>
