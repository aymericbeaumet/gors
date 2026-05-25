<script lang="ts">
import { onMount, onDestroy, tick } from "svelte";
import * as monaco from "monaco-editor";
import { Terminal } from "xterm";
import { FitAddon } from "@xterm/addon-fit";
import {
	CompilerCancelledError,
	Go2RustCompiler,
	type CompileResult,
} from "../go2rust-compiler";
import { RustRunner, State, type State as VmState } from "../rust-runner";
import {
	formatConsoleLine,
	type ConsoleLine,
	type ConsoleLineType,
} from "./console-format";
import { parseRustcErrors } from "./rustc-errors";
import type { SourceMapIndex } from "./source-map-index";
import MonacoEditor from "./MonacoEditor.svelte";
import CopyButton from "./CopyButton.svelte";
import {
	gostdlibCoverage,
	gostdlibCoverageSummary,
	type GostdlibCoveragePackage,
	type GostdlibCoverageSymbol,
} from "./gostdlib-coverage";

const ANSI_RE = /\x1b\[[0-9;]*m/g;

type RunMode = "autorun" | "autocompile" | "autotranspile" | "manual";
type AppView = "playground" | "stdlib-report";

interface ModeOption {
	value: RunMode;
	label: string;
}

interface PipelineCache {
	goSource: string | null;
	rustCode: string | null;
	jobId: string | null;
	compiled: boolean;
}

const STATE_TITLES = {
	[State.INITIALIZING]: "VM initializing...",
	[State.DOWNLOADING]: "VM downloading...",
	[State.BOOTING]: "VM booting...",
	[State.READY]: "VM ready",
	[State.COMPILING]: "VM busy",
	[State.RUNNING]: "VM busy",
	[State.ERROR]: "VM error",
};

const MODES: ModeOption[] = [
	{ value: "autorun", label: "Transpile + Compile + Run" },
	{ value: "autocompile", label: "Transpile + Compile" },
	{ value: "autotranspile", label: "Transpile" },
	{ value: "manual", label: "Noop" },
];
const PIPELINE_DEBOUNCE_MS = 350;

let activeView: AppView = "playground";
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

function storedRunMode(): RunMode {
	const value = localStorage.getItem("gors:runMode");
	return MODES.some((mode) => mode.value === value)
		? (value as RunMode)
		: "autorun";
}

let runMode: RunMode = storedRunMode();
let vmState: VmState = State.INITIALIZING;
let vmOverlayVisible = false;
let consoleLines: ConsoleLine[] = [];

let storedRatio = parseFloat(localStorage.getItem("gors:heightRatio") ?? "");
let editorsFlex = isNaN(storedRatio) || storedRatio <= 0 ? 1.618 : storedRatio;

let goEditor: monaco.editor.IStandaloneCodeEditor | null = null;
let rustEditor: monaco.editor.IStandaloneCodeEditor | null = null;

let editorsEl: HTMLDivElement;
let consoleSectionEl: HTMLDivElement;
let vmTerminalEl: HTMLDivElement;

const go2rust = new Go2RustCompiler();
const runner = new RustRunner();
let pipelineGeneration = 0;
let pipelineDebounceTimer: ReturnType<typeof setTimeout> | null = null;
let queuedPipeline = false;

let sourceMap: SourceMapIndex | null = null;
let goDecorations: string[] = [];
let rustDecorations: string[] = [];

// Read-only enforcement for rust editor
let rustExpectedValue = "";

let transpiling = false;
let activePipelines = 0;

// xterm
let term: Terminal;
let fitAddon: FitAddon;

$: vmTitle = STATE_TITLES[vmState] || vmState;
$: vmStarted =
	vmState === State.READY ||
	vmState === State.COMPILING ||
	vmState === State.RUNNING;
$: if (vmOverlayVisible && vmStarted) {
	tick().then(() => {
		fitAddon.fit();
		term.focus();
	});
}
let prevRunMode = runMode;
$: if (runMode !== prevRunMode) {
	prevRunMode = runMode;
	localStorage.setItem("gors:runMode", runMode);
	if (initialized) {
		pipelineGeneration++;
		schedulePipeline(0);
	}
}
let prevActiveView: AppView = activeView;
$: if (activeView !== prevActiveView) {
	prevActiveView = activeView;
	if (activeView === "playground") {
		tick().then(() => {
			goEditor?.layout();
			rustEditor?.layout();
		});
	}
}
$: pipelineBusy = activePipelines > 0;
$: transpileDisabled = runMode !== "manual" || pipelineBusy;
$: compileDisabled =
	runMode === "autocompile" || runMode === "autorun" || pipelineBusy;
$: runDisabled = runMode === "autorun" || pipelineBusy;

// Console helpers
function conClear() {
	consoleLines = [];
}
function conLine(type: ConsoleLineType, text: string) {
	consoleLines = [...consoleLines, { type, text }];
}
function conCmd(text: string) {
	conLine("cmd", text);
}
function conOut(text: string) {
	if (text) conLine("out", text);
}
function conErr(text: string) {
	if (!text) return;
	const clean = text.replace(ANSI_RE, "");
	conLine("err", clean);
}
function getConsoleText() {
	return consoleLines.map((l) => l.text).join("\n");
}

function formatDuration(durationMs: number): string {
	return durationMs < 1000
		? `${Math.round(durationMs)}ms`
		: `${(durationMs / 1000).toFixed(2)}s`;
}

// Source map highlighting
function highlightFromGo(line: number, column: number) {
	if (!sourceMap || !sourceMap.success || !rustEditor) {
		clearRustHighlight();
		return;
	}
	const span = sourceMap.go_to_output(line, column);
	if (span.length === 4) {
		rustDecorations = rustEditor.deltaDecorations(rustDecorations, [
			{
				range: new monaco.Range(span[0], span[1], span[2], span[3]),
				options: { className: "source-map-highlight", isWholeLine: false },
			},
		]);
	} else {
		clearRustHighlight();
	}
}

function highlightFromRust(line: number, column: number) {
	if (!sourceMap || !sourceMap.success || !goEditor) {
		clearGoHighlight();
		return;
	}
	const span = sourceMap.output_to_go(line, column);
	if (span.length === 4) {
		goDecorations = goEditor.deltaDecorations(goDecorations, [
			{
				range: new monaco.Range(span[0], span[1], span[2], span[3]),
				options: { className: "source-map-highlight", isWholeLine: false },
			},
		]);
	} else {
		clearGoHighlight();
	}
}

function clearGoHighlight() {
	if (goEditor) goDecorations = goEditor.deltaDecorations(goDecorations, []);
}
function clearRustHighlight() {
	if (rustEditor)
		rustDecorations = rustEditor.deltaDecorations(rustDecorations, []);
}

// Pipeline – each step caches its result and is skipped when inputs haven't changed
let cache: PipelineCache = {
	goSource: null,
	rustCode: null,
	jobId: null,
	compiled: false,
};

function cancelScheduledPipeline() {
	if (pipelineDebounceTimer) {
		clearTimeout(pipelineDebounceTimer);
		pipelineDebounceTimer = null;
	}
}

function schedulePipeline(delay = PIPELINE_DEBOUNCE_MS) {
	cancelScheduledPipeline();
	if (!initialized || runMode === "manual") return;
	pipelineDebounceTimer = setTimeout(() => {
		pipelineDebounceTimer = null;
		runPipeline();
	}, delay);
}

function setRustValue(value: string) {
	rustExpectedValue = value;
	rustEditor?.getModel()?.setValue(value);
}

async function waitForVM() {
	if (
		runner.state !== State.READY &&
		runner.state !== State.COMPILING &&
		runner.state !== State.RUNNING
	) {
		const startedAt = performance.now();
		conOut("waiting for VM...");
		await new Promise<void>((resolve) => {
			const unsub = runner.onStateChange((state) => {
				if (state === State.READY) {
					unsub();
					resolve();
				}
			});
		});
		conOut(`VM ready in ${formatDuration(performance.now() - startedAt)}`);
	}
}

async function doTranspile() {
	if (!goEditor || !rustEditor) return null;
	const activeGoModel = goEditor.getModel();
	if (!activeGoModel) return null;
	const goCode = activeGoModel.getValue();
	if (cache.goSource === goCode && cache.rustCode !== null)
		return cache.rustCode;

	cache = { goSource: null, rustCode: null, jobId: null, compiled: false };
	++pipelineGeneration;
	const goModel = goEditor.getModel();
	const rustModel = rustEditor.getModel();
	if (!goModel || !rustModel) return null;

	setRustValue("");
	monaco.editor.setModelMarkers(goModel, "gors", []);
	monaco.editor.setModelMarkers(rustModel, "rustc", []);
	sourceMap = null;

	conCmd("$ gors build -o main.rs main.go");
	transpiling = true;
	await tick();
	const gen = pipelineGeneration;
	let goResult: CompileResult;
	try {
		goResult = await go2rust.compile(goCode);
	} catch (err) {
		transpiling = false;
		if (err instanceof CompilerCancelledError) return null;
		conErr(err instanceof Error ? err.message : String(err));
		return null;
	}
	transpiling = false;
	if (gen !== pipelineGeneration || goCode !== activeGoModel.getValue()) {
		return null;
	}

	if (!goResult.success) {
		const err = goResult.error;
		const loc = err.line > 0 ? `:${err.line}:${err.column}` : "";
		conErr(`main.go${loc}: ${err.kind}: ${err.message}`);
		if (err.line > 0) {
			const lines = goCode.split("\n");
			let endCol = err.endColumn || err.column + 1;
			if (err.line <= lines.length)
				endCol = Math.min(endCol, lines[err.line - 1].length + 1);
			monaco.editor.setModelMarkers(goModel, "gors", [
				{
					severity: monaco.MarkerSeverity.Error,
					message: err.message,
					startLineNumber: err.line,
					startColumn: err.column,
					endLineNumber: err.line,
					endColumn: endCol,
					source: "gors",
					code: err.kind,
				},
			]);
		}
		return null;
	}

	conOut(
		`gors transpiled in ${formatDuration(goResult.durationMs)}${
			goResult.cacheHit ? " (cached)" : ""
		}`,
	);
	setRustValue(goResult.rustCode);
	sourceMap = goResult.sourceMap;
	cache = {
		goSource: goCode,
		rustCode: goResult.rustCode,
		jobId: null,
		compiled: false,
	};
	return goResult.rustCode;
}

async function doCompile(rustCode: string) {
	if (!rustEditor) return null;
	if (cache.compiled && cache.jobId) return cache.jobId;

	const gen = pipelineGeneration;
	await waitForVM();
	if (gen !== pipelineGeneration) return null;

	conCmd("$ rustc -o main main.rs");
	const startedAt = performance.now();
	const result = await runner.compile(rustCode);
	if (gen !== pipelineGeneration) return null;
	if (result.cancelled) return null;
	if (typeof result.jobId !== "string") return null;

	const rustModel = rustEditor.getModel();
	if (!rustModel) return null;
	monaco.editor.setModelMarkers(rustModel, "rustc", []);

	if (!result.compile.success) {
		conErr(result.compile.stderr);
		monaco.editor.setModelMarkers(
			rustModel,
			"rustc",
			parseRustcErrors(result.compile.stderr, monaco.MarkerSeverity),
		);
		return null;
	}

	conOut(`rustc finished in ${formatDuration(performance.now() - startedAt)}`);
	cache.compiled = true;
	cache.jobId = result.jobId;
	return result.jobId;
}

async function doRun(jobId: string) {
	const gen = pipelineGeneration;
	conCmd("$ ./main");
	const startedAt = performance.now();
	const result = await runner.runJob(jobId);
	if (gen !== pipelineGeneration) return;
	if (result.cancelled) return;
	if (!result.run) return;

	conOut(result.run.stdout);
	conErr(result.run.stderr);
	if (result.run.exitCode !== 0 && !result.run.stderr) {
		conErr(`program exited with code ${result.run.exitCode}`);
	}
	conOut(`run finished in ${formatDuration(performance.now() - startedAt)}`);
}

async function runPipeline() {
	if (runMode === "manual") return;
	if (activePipelines > 0) {
		queuedPipeline = true;
		return;
	}
	activePipelines++;
	try {
		conClear();
		const rustCode = await doTranspile();
		if (!rustCode) return;
		if (runMode === "autorun") {
			const jobId = await doCompile(rustCode);
			if (jobId) await doRun(jobId);
		} else if (runMode === "autocompile") {
			await doCompile(rustCode);
		}
	} finally {
		activePipelines--;
		if (queuedPipeline) {
			queuedPipeline = false;
			schedulePipeline(0);
		}
	}
}

function onGoChanged() {
	pipelineGeneration++;
	if (activePipelines > 0) {
		go2rust.cancelActive("compiler input changed");
	}
	schedulePipeline();
}

async function handleTranspile() {
	cancelScheduledPipeline();
	pipelineGeneration++;
	activePipelines++;
	try {
		conClear();
		await doTranspile();
	} finally {
		activePipelines--;
	}
}
async function handleCompile() {
	cancelScheduledPipeline();
	pipelineGeneration++;
	activePipelines++;
	try {
		conClear();
		const rustCode = await doTranspile();
		if (rustCode) await doCompile(rustCode);
	} finally {
		activePipelines--;
	}
}
async function handleRun() {
	cancelScheduledPipeline();
	pipelineGeneration++;
	activePipelines++;
	try {
		conClear();
		const rustCode = await doTranspile();
		if (!rustCode) return;
		const jobId = await doCompile(rustCode);
		if (!jobId) return;
		await doRun(jobId);
	} finally {
		activePipelines--;
	}
}

// Resize
function onResizeMousedown(e: MouseEvent) {
	e.preventDefault();
	const startY = e.clientY;
	const startEH = editorsEl.offsetHeight;
	const startCH = consoleSectionEl.offsetHeight;
	const total = startEH + startCH;
	editorsEl.style.flex = "none";
	consoleSectionEl.style.flex = "none";
	editorsEl.style.height = startEH + "px";
	consoleSectionEl.style.height = startCH + "px";
	function onMove(ev: MouseEvent) {
		const h = Math.min(
			total - 200,
			Math.max(200, startEH + ev.clientY - startY),
		);
		editorsEl.style.height = h + "px";
		consoleSectionEl.style.height = total - h + "px";
	}
	function onUp() {
		document.removeEventListener("mousemove", onMove);
		document.removeEventListener("mouseup", onUp);
		const eh = editorsEl.offsetHeight;
		const ch = consoleSectionEl.offsetHeight;
		editorsFlex = ch > 0 ? eh / ch : 1.618;
		localStorage.setItem("gors:heightRatio", editorsFlex.toString());
		editorsEl.style.height = "";
		editorsEl.style.flex = "";
		consoleSectionEl.style.height = "";
		consoleSectionEl.style.flex = "";
	}
	document.addEventListener("mousemove", onMove);
	document.addEventListener("mouseup", onUp);
}

// VM terminal overlay
function openVmOverlay() {
	vmOverlayVisible = true;
}

function closeVmOverlay() {
	vmOverlayVisible = false;
}

function onOverlayClick(e: MouseEvent) {
	if (e.target === e.currentTarget) closeVmOverlay();
}

function onKeydown(e: KeyboardEvent) {
	if (e.key === "Escape" && vmOverlayVisible) closeVmOverlay();
}

let resizeObserver: ResizeObserver | null = null;

onMount(() => {
	// xterm
	term = new Terminal({
		fontSize: 12,
		fontFamily: "'Fira Code Variable', 'Fira Code', monospace",
		theme: { background: "#0d1117", foreground: "#c9d1d9" },
		convertEol: true,
		scrollback: 5000,
		cursorStyle: "bar",
		cursorBlink: true,
	});
	fitAddon = new FitAddon();
	term.loadAddon(fitAddon);
	term.open(vmTerminalEl);
	term.onData((data) => runner.sendSerial(data));

	let serialByteQueue: number[] = [];
	let serialFlushTimer: ReturnType<typeof setTimeout> | null = null;
	runner.onSerialByte((byte) => {
		serialByteQueue.push(byte);
		if (!serialFlushTimer) {
			serialFlushTimer = setTimeout(() => {
				if (serialByteQueue.length > 0)
					term.write(new Uint8Array(serialByteQueue));
				serialByteQueue = [];
				serialFlushTimer = null;
			}, 50);
		}
	});

	// VM state
	runner.onStateChange((state) => {
		vmState = state;
	});
	runner.start().catch(() => {
		vmState = State.ERROR;
	});

	// Resize observer for terminal
	resizeObserver = new ResizeObserver(() => {
		if (vmOverlayVisible) fitAddon.fit();
	});
	resizeObserver.observe(vmTerminalEl);

	// Rust hover provider
	monaco.languages.registerHoverProvider("rust", {
		provideHover(model, position) {
			const markers = monaco.editor.getModelMarkers({ resource: model.uri });
			for (const m of markers) {
				if (
					position.lineNumber >= m.startLineNumber &&
					position.lineNumber <= m.endLineNumber &&
					position.column >= m.startColumn &&
					position.column <= m.endColumn
				) {
					return {
						range: new monaco.Range(
							m.startLineNumber,
							m.startColumn,
							m.endLineNumber,
							m.endColumn,
						),
						contents: [{ value: `**${m.source}(${m.code})**: ${m.message}` }],
					};
				}
			}
			return null;
		},
	});
});

// Wire up editors after they're bound
let goEditorReady = false;
let rustEditorReady = false;

$: if (goEditor && !goEditorReady) {
	goEditorReady = true;
	goEditor.onMouseMove((e: monaco.editor.IEditorMouseEvent) => {
		if (e.target.position)
			highlightFromGo(e.target.position.lineNumber, e.target.position.column);
	});
	goEditor.onMouseLeave(() => clearRustHighlight());
}

$: if (rustEditor && !rustEditorReady) {
	rustEditorReady = true;
	rustEditor.onMouseMove((e: monaco.editor.IEditorMouseEvent) => {
		if (e.target.position)
			highlightFromRust(e.target.position.lineNumber, e.target.position.column);
	});
	rustEditor.onMouseLeave(() => clearGoHighlight());

	// Read-only enforcement
	rustEditor.getModel()?.onDidChangeContent(() => {
		const model = rustEditor?.getModel();
		if (!model) return;
		const current = model.getValue();
		if (current !== rustExpectedValue) {
			const markers = monaco.editor.getModelMarkers({
				resource: model.uri,
			});
			model.setValue(rustExpectedValue);
			monaco.editor.setModelMarkers(model, "rustc", markers);
		}
	});
}

$: if (goEditor && rustEditor && !initialized) {
	initialized = true;
	goEditor.focus();
	goEditor
		.getModel()
		?.setValue(
			[
				"package main",
				"",
				'import "fmt"',
				"",
				"func main() {",
				'\tfmt.Println("Hello, World!")',
				"}",
			].join("\n"),
		);
	goEditor.setPosition({ lineNumber: 6, column: 2 });
}

let initialized = false;

onDestroy(() => {
	cancelScheduledPipeline();
	go2rust.dispose();
	resizeObserver?.disconnect();
	term?.dispose();
});
</script>

<svelte:window on:keydown={onKeydown} />

<main>
  <header>
    <h1><a href="https://github.com/aymericbeaumet/gors" target="_blank" rel="noopener">gors</a></h1>
    <p class="subtitle">Go toolchain written in Rust (parser, compiler, sandbox)</p>
    <nav class="view-switch" aria-label="App view">
      <button type="button" class:active={activeView === "playground"} aria-pressed={activeView === "playground"} on:click={() => activeView = "playground"}>Playground</button>
      <button type="button" class:active={activeView === "stdlib-report"} aria-pressed={activeView === "stdlib-report"} on:click={() => activeView = "stdlib-report"}>Stdlib report</button>
    </nav>
    <div class="spacer"></div>
    <div class="mode-group">
      <span class="mode-label">Mode:</span>
      <select bind:value={runMode} class="run-mode-select" title="Pipeline mode">
        {#each MODES as mode}
          <option value={mode.value}>{mode.label}</option>
        {/each}
      </select>
    </div>
    <!-- svelte-ignore a11y-click-events-have-key-events -->
    <div
      class="vm-status"
      data-state={vmState}
      title={vmTitle}
      on:click={openVmOverlay}
      role="button"
      tabindex="0"
    >
      <span class="vm-dot"></span>
      <span class="vm-label">Linux VM</span>
    </div>
  </header>

  <div class="content" class:hidden={activeView !== "playground"} aria-hidden={activeView !== "playground"}>
    <div class="editors" bind:this={editorsEl} style="flex: {editorsFlex}">
      <div class="editor-container go">
        <div class="editor-header">
          <div class="label"><span class="dot"></span><span>main.go</span></div>
          <div class="actions">
            <button class="action-button" title="Transpile with gors" on:click={handleTranspile} disabled={transpileDisabled}>
              {#if transpiling}
                <span class="btn-spinner"></span>
              {:else}
                <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
                  <path d="M16 1H4c-1.1 0-2 .9-2 2v14h2V3h12V1zm-1 4l6 6v10c0 1.1-.9 2-2 2H7.99C6.89 21 6 20.1 6 19l.01-14c0-1.1.89-2 1.99-2h7zm-1 7h5.5L14 6.5V12z"/>
                </svg>
              {/if}
              <span>Transpile</span>
            </button>
            <CopyButton getContent={() => goEditor?.getModel()?.getValue()} title="Copy Go code" />
          </div>
        </div>
        <div class="editor-wrapper">
          <MonacoEditor language="go" bind:editor={goEditor} on:change={onGoChanged} />
        </div>
      </div>

      <div class="editor-container rust">
        <div class="editor-header">
          <div class="label"><span class="dot"></span><span>main.rs</span></div>
          <div class="actions">
            <button class="action-button" title="Compile with rustc (in the Linux VM)" on:click={handleCompile} disabled={compileDisabled}>
              {#if vmState === State.COMPILING}
                <span class="btn-spinner"></span>
              {:else}
                <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
                  <path d="M9.4 16.6L4.8 12l4.6-4.6L8 6l-6 6 6 6 1.4-1.4zm5.2 0L19.2 12l-4.6-4.6L16 6l6 6-6 6-1.4-1.4z"/>
                </svg>
              {/if}
              <span>Compile</span>
            </button>
            <button class="action-button" title="Run (in the Linux VM)" on:click={handleRun} disabled={runDisabled}>
              {#if vmState === State.RUNNING}
                <span class="btn-spinner"></span>
              {:else}
                <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
                  <path d="M8 5v14l11-7z"/>
                </svg>
              {/if}
              <span>Run</span>
            </button>
            <CopyButton getContent={() => rustEditor?.getModel()?.getValue()} title="Copy Rust code" />
          </div>
        </div>
        <div class="editor-wrapper">
          <MonacoEditor language="rust" bind:editor={rustEditor} />
        </div>
      </div>
    </div>

    <!-- svelte-ignore a11y-no-static-element-interactions -->
    <div class="resize-handle" on:mousedown={onResizeMousedown}></div>

    <div class="console-section" bind:this={consoleSectionEl}>
      <div class="console-header">
        <div class="console-left">
          <div class="console-title"><span class="dot"></span><span>Console</span></div>
        </div>
        <div class="console-right">
          <CopyButton getContent={getConsoleText} title="Copy console output" />
        </div>
      </div>
      <pre class="console-content">{#each consoleLines as line}<span class={line.type}>{@html formatConsoleLine(line)}</span>{'\n'}{/each}</pre>
    </div>
  </div>

  <section class="content stdlib-report-view" class:hidden={activeView !== "stdlib-report"} aria-hidden={activeView !== "stdlib-report"}>
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
              <code>{item.packagePath}</code>
              <span class:tested={item.tested} class:untested={!item.tested}>{item.testedSymbolCount}/{item.symbolCount} tested</span>
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
                <code>{fixture}</code>
              {/each}
            </div>
          </div>
        {:else}
          <div class="report-empty">No matching coverage</div>
        {/each}
      </div>
    </div>
  </section>
</main>

<!-- svelte-ignore a11y-click-events-have-key-events -->
<!-- svelte-ignore a11y-no-static-element-interactions -->
<div class="vm-terminal-overlay" class:visible={vmOverlayVisible} on:click={onOverlayClick}>
  <div class="vm-terminal-panel">
    <div class="vm-terminal-header">
      <div class="vm-terminal-left"></div>
      <span class="vm-terminal-title">Linux VM</span>
      <div class="vm-terminal-right">
        <button class="vm-terminal-close" title="Close" on:click={closeVmOverlay}>&times;</button>
      </div>
    </div>
    {#if !vmStarted}
      <div class="vm-spinner-container">
        <div class="vm-spinner"></div>
        <span class="vm-spinner-label">{vmTitle}</span>
      </div>
    {/if}
    <div class="vm-terminal-body" bind:this={vmTerminalEl} style:display={vmStarted ? '' : 'none'}></div>
  </div>
</div>

<style>
  :global(*) { box-sizing: border-box; }
  :global(html), :global(body), :global(#app) { height: 100%; overflow: hidden; }
  :global(body) {
    margin: 0; padding: 0;
    font-family: system-ui, -apple-system, sans-serif;
    background: #0d1117; color: #c9d1d9;
  }
  :global(.source-map-highlight) {
    background-color: rgba(88, 166, 255, 0.2);
    border-radius: 2px;
  }
  :global(.monaco-editor .lines-content) { padding-left: 5px; }
  :global(.monaco-editor .editor-widget) { z-index: 50; }

  main {
    height: 100%;
    display: flex; flex-direction: column;
  }

  header {
    display: flex; align-items: center; gap: 12px;
    padding: 8px 16px; flex-shrink: 0;
    border-bottom: 1px solid #30363d;
  }

  .content {
    flex: 1; display: flex; flex-direction: column;
    padding: 16px; min-height: 0;
  }
  header h1 { margin: 0; font-size: 16px; font-weight: 600; }
  header h1 a { color: #c9d1d9; text-decoration: none; }
  header h1 a:hover { color: #58a6ff; }
  .subtitle { margin: 0; font-size: 13px; color: #8b949e; }
  .spacer { flex: 1; }

  .view-switch {
    display: flex; align-items: center; gap: 2px;
    padding: 2px; border: 1px solid #30363d; border-radius: 6px;
    background: #0d1117;
  }
  .view-switch button {
    min-width: 96px; height: 26px; padding: 0 10px;
    border: 0; border-radius: 4px; background: transparent;
    color: #8b949e; font: inherit; font-size: 12px;
    cursor: pointer;
  }
  .view-switch button:hover { color: #c9d1d9; background: #21262d; }
  .view-switch button.active { color: #0d1117; background: #58a6ff; }

  .hidden { display: none !important; }

  .run-mode-select {
    padding: 4px 8px; background: transparent;
    border: 1px solid #30363d; border-radius: 4px;
    color: #8b949e; font-size: 11px; font-family: inherit;
    cursor: pointer; transition: all 0.15s ease; appearance: auto;
  }
  .run-mode-select:hover { background: #21262d; border-color: #8b949e; color: #c9d1d9; }
  .run-mode-select:focus { outline: none; border-color: #58a6ff; }
  .run-mode-select option { background: #161b22; color: #c9d1d9; }

  .vm-status {
    display: flex; align-items: center; gap: 6px;
    padding: 4px 8px; border-radius: 4px; font-size: 11px;
    user-select: none; transition: all 0.15s ease;
    border: 1px solid transparent; font-weight: 400; font-family: inherit;
    cursor: pointer;
  }
  .vm-status:hover { border-color: #30363d; }

  .vm-dot {
    width: 6px; height: 6px; border-radius: 50%; flex-shrink: 0;
  }
  [data-state="initializing"] > .vm-dot,
  [data-state="downloading"] > .vm-dot { background: #d29922; }
  [data-state="booting"] > .vm-dot { background: #d29922; animation: pulse 1.5s ease-in-out infinite; }
  [data-state="ready"] > .vm-dot { background: #3fb950; }
  [data-state="compiling"] > .vm-dot,
  [data-state="running"] > .vm-dot { background: #58a6ff; animation: pulse 0.8s ease-in-out infinite; }
  [data-state="error"] > .vm-dot { background: #f85149; }

  [data-state="initializing"] > .vm-label,
  [data-state="downloading"] > .vm-label { color: #d29922; }
  [data-state="booting"] > .vm-label { color: #d29922; }
  [data-state="ready"] > .vm-label { color: #3fb950; }
  [data-state="compiling"] > .vm-label,
  [data-state="running"] > .vm-label { color: #58a6ff; }
  [data-state="error"] > .vm-label { color: #f85149; }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.3; }
  }

  .editors { display: flex; gap: 16px; min-height: 200px; }

  .mode-group { display: flex; align-items: center; gap: 6px; }
  .mode-label { font-size: 11px; color: #8b949e; }

  .editor-container {
    flex: 1; display: flex; flex-direction: column; min-width: 0;
  }

  .editor-header {
    display: flex; align-items: center; justify-content: space-between;
    padding: 8px 12px; background: #161b22;
    border-radius: 8px 8px 0 0; font-size: 12px; font-weight: 600; height: 36px;
  }

  .editor-header .label { display: flex; align-items: center; gap: 8px; }

  .editor-header .dot { width: 8px; height: 8px; border-radius: 50%; }
  .go .dot { background: #00ADD8; }
  .rust .dot { background: #FFC832; }

  .actions { display: flex; align-items: center; gap: 8px; }

  .editor-wrapper {
    flex: 1; position: relative;
    border: 2px solid; border-top: none; border-radius: 0 0 8px 8px; overflow: visible;
  }
  .go .editor-wrapper { border-color: #00ADD8; }
  .rust .editor-wrapper { border-color: #FFC832; }

  .action-button {
    padding: 4px 8px; background: transparent;
    border: 1px solid #30363d; border-radius: 4px;
    color: #8b949e; font-size: 11px; font-family: inherit;
    cursor: pointer; display: flex; align-items: center; gap: 4px;
    transition: all 0.15s ease;
  }
  .action-button:hover:not(:disabled) { background: #21262d; border-color: #8b949e; color: #c9d1d9; }
  .action-button:disabled { opacity: 0.35; cursor: not-allowed; }
  .action-button svg { width: 12px; height: 12px; fill: currentColor; }
  .btn-spinner {
    width: 12px; height: 12px; border-radius: 50%; flex-shrink: 0;
    border: 2px solid #30363d; border-top-color: #8b949e;
    animation: spin 0.8s linear infinite;
  }

  .resize-handle {
    height: 6px; margin: 5px 0; cursor: row-resize;
    display: flex; align-items: center; justify-content: center;
    flex-shrink: 0; user-select: none;
  }
  .resize-handle::after {
    content: ''; width: 40px; height: 3px; border-radius: 2px;
    background: #30363d; transition: background 0.15s;
  }
  .resize-handle:hover::after { background: #484f58; }

  .console-section {
    border: 2px solid #30363d; border-radius: 8px; overflow: hidden;
    flex: 1; min-height: 200px; display: flex; flex-direction: column;
  }

  .console-header {
    display: flex; align-items: center; justify-content: space-between;
    padding: 8px 12px; background: #161b22;
    font-size: 12px; font-weight: 600; height: 36px; flex-shrink: 0;
  }
  .console-left { display: flex; align-items: center; gap: 12px; }
  .console-right { display: flex; align-items: center; gap: 8px; }
  .console-title { display: flex; align-items: center; gap: 8px; }
  .console-title .dot { width: 8px; height: 8px; border-radius: 50%; background: #c9d1d9; }

  .console-content {
    flex: 1; margin: 12px; background: #0d1117;
    font-family: 'Fira Code Variable', 'Fira Code', monospace;
    font-size: 13px; line-height: 1.5; overflow-y: auto;
    white-space: pre-wrap; word-break: break-all; color: #c9d1d9;
  }
  .console-content :global(.cmd) { color: #c9d1d9; }
  .console-content :global(.err) { color: #f85149; }
  .console-content :global(.out) { color: #c9d1d9; }
  .console-content :global(a) { color: #58a6ff; text-decoration: none; }
  .console-content :global(a:hover) { text-decoration: underline; }

  .stdlib-report-view {
    gap: 14px;
  }

  .report-summary {
    display: grid;
    grid-template-columns: repeat(4, minmax(120px, 170px)) minmax(260px, 1fr);
    gap: 12px;
    flex-shrink: 0;
  }

  .report-metric,
  .report-filter {
    min-height: 58px;
    border: 1px solid #30363d;
    border-radius: 8px;
    background: #161b22;
  }

  .report-metric {
    display: flex;
    flex-direction: column;
    justify-content: center;
    padding: 8px 12px;
  }

  .report-metric strong {
    font-size: 22px;
    line-height: 1;
    color: #c9d1d9;
    font-weight: 650;
  }

  .report-metric span,
  .report-filter span {
    margin-top: 4px;
    color: #8b949e;
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
    border: 1px solid #30363d;
    border-radius: 4px;
    background: #0d1117;
    color: #c9d1d9;
    font: inherit;
    font-size: 13px;
  }

  .report-filter input:focus {
    outline: none;
    border-color: #58a6ff;
  }

  .report-list {
    flex: 1;
    min-height: 0;
    border: 1px solid #30363d;
    border-radius: 8px;
    overflow: hidden;
    background: #0d1117;
    display: flex;
    flex-direction: column;
  }

  .report-list-head,
  .coverage-row {
    display: grid;
    grid-template-columns: minmax(180px, 0.8fr) minmax(320px, 2fr) minmax(200px, 1fr);
    column-gap: 16px;
    align-items: start;
  }

  .report-list-head {
    padding: 10px 14px;
    background: #161b22;
    color: #8b949e;
    font-size: 12px;
    font-weight: 600;
    border-bottom: 1px solid #30363d;
  }

  .report-list-body {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }

  .coverage-row {
    padding: 12px 14px;
    border-bottom: 1px solid #21262d;
  }

  .coverage-row:last-child { border-bottom: 0; }

  .package-cell {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }

  .package-cell code {
    color: #7ee787;
    font-family: 'Fira Code Variable', 'Fira Code', monospace;
    font-size: 13px;
    word-break: break-word;
  }

  .package-cell span {
    color: #8b949e;
    font-size: 12px;
  }

  .package-cell span.tested { color: #3fb950; }
  .package-cell span.untested { color: #f85149; }

  .symbol-cell,
  .fixture-cell {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    min-width: 0;
  }

  .symbol-token,
  .fixture-cell code {
    max-width: 100%;
    padding: 3px 7px;
    border: 1px solid #30363d;
    border-radius: 4px;
    background: #161b22;
    color: #c9d1d9;
    font-family: 'Fira Code Variable', 'Fira Code', monospace;
    font-size: 12px;
    line-height: 1.35;
    overflow-wrap: anywhere;
  }

  .symbol-token {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .symbol-token.tested {
    border-color: #2ea043;
    background: rgba(46, 160, 67, 0.12);
  }
  .symbol-token.untested {
    border-color: #3d444d;
    color: #8b949e;
  }
  .symbol-token small {
    color: inherit;
    opacity: 0.7;
    font-size: 10px;
    line-height: 1;
  }
  .fixture-cell code { color: #d2a8ff; }

  .report-empty {
    padding: 28px 14px;
    color: #8b949e;
    font-size: 13px;
  }

  .vm-terminal-overlay {
    display: none;
    position: fixed; inset: 0; background: rgba(0,0,0,0.6); z-index: 100;
  }
  .vm-terminal-overlay.visible {
    display: flex; align-items: center; justify-content: center; padding: 48px;
  }
  .vm-terminal-panel {
    width: 800px; height: 600px; background: #0d1117;
    border: 1px solid #30363d; border-radius: 12px; overflow: hidden;
    display: flex; flex-direction: column;
  }
  .vm-terminal-header {
    display: grid; grid-template-columns: 1fr auto 1fr; align-items: center;
    padding: 10px 12px; background: #161b22; border-bottom: 1px solid #21262d;
  }
  .vm-terminal-left { min-width: 0; }
  .vm-terminal-title { font-size: 13px; font-weight: 600; text-align: center; }
  .vm-terminal-right { display: flex; justify-content: flex-end; }
  .vm-terminal-close {
    padding: 4px 8px; background: transparent; border: none;
    color: #8b949e; font-size: 16px; cursor: pointer; border-radius: 4px;
  }
  .vm-terminal-close:hover { background: #21262d; color: #c9d1d9; }
  .vm-terminal-body { flex: 1; overflow: hidden; padding: 8px; }

  .vm-spinner-container {
    flex: 1; display: flex; flex-direction: column;
    align-items: center; justify-content: center; gap: 16px;
  }
  .vm-spinner {
    width: 32px; height: 32px; border-radius: 50%;
    border: 3px solid #30363d; border-top-color: #58a6ff;
    animation: spin 0.8s linear infinite;
  }
  .vm-spinner-label { font-size: 13px; color: #8b949e; }
  @keyframes spin { to { transform: rotate(360deg); } }

  @media (max-width: 900px) {
    :global(html), :global(body), :global(#app) { overflow: auto; }
    header { flex-wrap: wrap; }
    .subtitle { display: none; }
    .spacer { display: none; }
    .content { min-height: calc(100vh - 88px); }
    .editors { flex-direction: column; }
    .report-summary { grid-template-columns: repeat(2, minmax(0, 1fr)); }
    .report-filter { grid-column: 1 / -1; }
    .report-list-head { display: none; }
    .coverage-row {
      grid-template-columns: 1fr;
      gap: 10px;
    }
  }
</style>
