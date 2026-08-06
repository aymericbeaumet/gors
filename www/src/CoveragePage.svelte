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
	readonly summary: ConformanceSummary;
	readonly cases: readonly ConformanceCase[];
}

interface ConformanceReport {
	readonly title: string;
	readonly source: {
		readonly url: string;
		readonly languageVersion: string;
	};
	readonly summary: ConformanceSummary;
	readonly groups: readonly ConformanceGroup[];
}

const reports: readonly {
	readonly label: string;
	readonly shortLabel: string;
	readonly fixtureSet: "go_spec" | "go_stdlib";
	readonly report: ConformanceReport;
}[] = [
	{
		label: "Go specification",
		shortLabel: "Spec",
		fixtureSet: "go_spec",
		report: goSpecReport as ConformanceReport,
	},
	{
		label: "Go standard library",
		shortLabel: "Stdlib",
		fixtureSet: "go_stdlib",
		report: goStdlibReport as ConformanceReport,
	},
];

let reportIndex = 0;
let selectedGroupId = reports[0].report.groups[0]?.id ?? "";
let query = "";

$: current = reports[reportIndex];
$: groups = current.report.groups.filter((group) =>
	group.title.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()),
);
$: selectedGroup =
	current.report.groups.find((group) => group.id === selectedGroupId) ??
	groups[0] ??
	current.report.groups[0];

function selectReport(index: number) {
	reportIndex = index;
	query = "";
	selectedGroupId = reports[index].report.groups[0]?.id ?? "";
}

function percentage(passing: number, total: number): string {
	return total === 0 ? "0.0%" : `${((passing / total) * 100).toFixed(1)}%`;
}

function fixtureUrl(fixture: string): string {
	return `${FIXTURE_GITHUB_BASE}/${current.fixtureSet}/${fixture}`;
}
</script>

<section class="coverage-page">
  <header class="coverage-hero">
    <div>
      <p class="eyebrow">Continuously measured</p>
      <h1>Go compatibility</h1>
      <p class="summary">
        Every published result comes from executable fixtures compared with the repository-pinned Go toolchain. Explore exact language tests and standard-library symbols below.
      </p>
    </div>
    <div class="report-tabs" role="tablist" aria-label="Compatibility reports">
      {#each reports as item, index}
        <button
          type="button"
          role="tab"
          aria-selected={reportIndex === index}
          class:active={reportIndex === index}
          on:click={() => selectReport(index)}
        >
          <span>{item.shortLabel}</span>
          <strong>{percentage(item.report.summary.passingCaseCount, item.report.summary.caseCount)}</strong>
        </button>
      {/each}
    </div>
  </header>

  <div class="report-heading">
    <div>
      <h2>{current.report.title}</h2>
      <a href={current.report.source.url} target="_blank" rel="noopener">
        Go {current.report.source.languageVersion.replace(/^go/, "")}
      </a>
    </div>
    <div class="metrics" aria-label={`${current.label} results`}>
      <div><strong>{current.report.summary.passingCaseCount}/{current.report.summary.caseCount}</strong><span>tests covered</span></div>
      <div><strong>{current.report.summary.passingGroupCount}/{current.report.summary.groupCount}</strong><span>groups complete</span></div>
      <div><strong>{current.report.summary.fixtureCount}</strong><span>passing fixtures</span></div>
    </div>
  </div>

  <div class="report-browser">
    <aside>
      <label>
        <span>Filter {current.fixtureSet === "go_spec" ? "categories" : "packages"}</span>
        <input bind:value={query} type="search" placeholder="Type to filter…" />
      </label>
      <div class="group-list">
        {#each groups as group}
          <button
            type="button"
            class:selected={selectedGroup?.id === group.id}
            on:click={() => (selectedGroupId = group.id)}
          >
            <code>{group.title}</code>
            <span>{group.summary.passingCaseCount}/{group.summary.caseCount}</span>
          </button>
        {:else}
          <p class="empty">No matching groups.</p>
        {/each}
      </div>
    </aside>

    <section class="case-panel" aria-live="polite">
      {#if selectedGroup}
        <div class="case-heading">
          <div>
            <h3>{selectedGroup.title}</h3>
            <p>{percentage(selectedGroup.summary.passingCaseCount, selectedGroup.summary.caseCount)} covered</p>
          </div>
          <strong>{selectedGroup.summary.passingCaseCount}/{selectedGroup.summary.caseCount}</strong>
        </div>
        <div class="case-list">
          {#each selectedGroup.cases as item}
            <article class:passing={item.status === "passing"}>
              <div class="case-copy">
                <strong>{item.title}</strong>
                <span>{item.subtitle || item.kind}</span>
                {#if item.reason}<small>{item.reason}</small>{/if}
                {#if item.fixtures.length > 0}
                  <div class="fixtures">
                    {#each item.fixtures as fixture}
                      <a href={fixtureUrl(fixture)} target="_blank" rel="noopener"><code>{fixture}</code></a>
                    {/each}
                  </div>
                {/if}
              </div>
              <span class="status">{item.status === "passing" ? "Covered" : "Open"}</span>
            </article>
          {/each}
        </div>
      {:else}
        <p class="empty">Select a group to inspect its tests.</p>
      {/if}
    </section>
  </div>
</section>

<style>
  .coverage-page { display: grid; gap: 20px; width: min(1440px, 100%); margin: 0 auto; padding: 40px 24px 64px; color: #1f2328; }
  .coverage-hero { display: flex; align-items: end; justify-content: space-between; gap: 28px; }
  .eyebrow { margin: 0 0 6px; color: #0969da; font-size: 13px; font-weight: 750; letter-spacing: .08em; text-transform: uppercase; }
  h1, h2, h3, p { margin: 0; }
  h1 { font-size: clamp(34px, 5vw, 58px); line-height: 1; }
  .summary { max-width: 760px; margin-top: 12px; color: #57606a; font-size: 16px; line-height: 1.55; }
  .report-tabs { display: flex; gap: 8px; }
  .report-tabs button { display: grid; min-width: 116px; gap: 3px; padding: 11px 14px; border: 1px solid #d0d7de; border-radius: 10px; background: #fff; color: #57606a; cursor: pointer; text-align: left; }
  .report-tabs button.active { border-color: #0969da; box-shadow: 0 0 0 1px #0969da; color: #0969da; }
  .report-tabs strong { color: #1f2328; font-size: 18px; }
  .report-heading { display: flex; align-items: center; justify-content: space-between; gap: 24px; padding: 20px 22px; border: 1px solid #d0d7de; border-radius: 12px; background: #fff; }
  .report-heading h2 { font-size: 22px; }
  a { color: #0969da; text-decoration: none; }
  a:hover { text-decoration: underline; }
  .report-heading a { display: inline-block; margin-top: 5px; font-size: 13px; }
  .metrics { display: flex; gap: 28px; }
  .metrics div { display: grid; gap: 2px; }
  .metrics strong { font-size: 18px; }
  .metrics span { color: #57606a; font-size: 12px; }
  .report-browser { display: grid; grid-template-columns: minmax(230px, 320px) minmax(0, 1fr); min-height: 580px; overflow: hidden; border: 1px solid #d0d7de; border-radius: 12px; background: #fff; }
  aside { display: grid; min-height: 0; grid-template-rows: auto minmax(0, 1fr); border-right: 1px solid #d0d7de; background: #f6f8fa; }
  label { display: grid; gap: 7px; padding: 16px; border-bottom: 1px solid #d0d7de; color: #57606a; font-size: 12px; font-weight: 650; }
  input { min-width: 0; padding: 9px 10px; border: 1px solid #d0d7de; border-radius: 7px; background: #fff; color: #1f2328; font: inherit; }
  input:focus { border-color: #0969da; outline: 2px solid #0969da22; }
  .group-list { min-height: 0; max-height: 680px; overflow: auto; padding: 8px; }
  .group-list button { display: flex; width: 100%; align-items: center; justify-content: space-between; gap: 10px; padding: 9px 10px; border: 0; border-radius: 6px; background: transparent; color: #1f2328; cursor: pointer; text-align: left; }
  .group-list button:hover { background: #eaeef2; }
  .group-list button.selected { background: #ddf4ff; color: #0969da; }
  .group-list code { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .group-list span { flex-shrink: 0; color: #57606a; font-size: 11px; }
  .case-panel { min-width: 0; }
  .case-heading { display: flex; align-items: center; justify-content: space-between; padding: 18px 20px; border-bottom: 1px solid #d0d7de; }
  .case-heading h3 { font-size: 20px; }
  .case-heading p { margin-top: 3px; color: #57606a; font-size: 13px; }
  .case-heading > strong { font-size: 20px; }
  .case-list { display: grid; max-height: 680px; overflow: auto; }
  article { display: flex; align-items: start; justify-content: space-between; gap: 20px; padding: 15px 20px; border-bottom: 1px solid #d8dee4; border-left: 3px solid #d0d7de; }
  article.passing { border-left-color: #1a7f37; }
  .case-copy { display: grid; min-width: 0; gap: 3px; }
  .case-copy > span, small { color: #57606a; }
  small { max-width: 900px; margin-top: 4px; line-height: 1.4; }
  .status { flex-shrink: 0; padding: 3px 7px; border-radius: 999px; background: #f6f8fa; color: #57606a; font-size: 11px; font-weight: 700; }
  article.passing .status { background: #dafbe1; color: #1a7f37; }
  .fixtures { display: flex; flex-wrap: wrap; gap: 5px; margin-top: 6px; }
  .fixtures a { padding: 2px 5px; border-radius: 4px; background: #ddf4ff; font-size: 11px; }
  .empty { padding: 20px; color: #57606a; }
  @media (max-width: 820px) { .coverage-page { padding: 28px 14px 48px; } .coverage-hero, .report-heading { align-items: stretch; flex-direction: column; } .metrics { justify-content: space-between; gap: 10px; } .report-tabs button { min-width: 104px; } .report-browser { grid-template-columns: 1fr; } aside { border-right: 0; border-bottom: 1px solid #d0d7de; } .group-list { max-height: 240px; } }
</style>
