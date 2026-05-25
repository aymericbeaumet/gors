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
import CoveragePage from "./CoveragePage.svelte";

const ANSI_RE = /\x1b\[[0-9;]*m/g;

type AppRoute = "home" | "playground" | "coverage";

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

const PIPELINE_DEBOUNCE_MS = 350;
function routeFromPath(pathname: string): AppRoute {
	const normalized = pathname.replace(/\/+$/, "");
	if (normalized === "/coverage") return "coverage";
	if (normalized === "/playground") return "playground";
	return "home";
}

function pathForRoute(nextRoute: AppRoute): string {
	if (nextRoute === "coverage") return "/coverage";
	if (nextRoute === "playground") return "/playground";
	return "/";
}

function layoutEditors() {
	tick().then(() => {
		goEditor?.layout();
		rustEditor?.layout();
	});
}

function scrollPageToTop() {
	tick().then(() => {
		window.scrollTo({ top: 0, left: 0 });
	});
}

let route: AppRoute = routeFromPath(window.location.pathname);

function navigateTo(nextRoute: AppRoute, event?: MouseEvent) {
	event?.preventDefault();
	const nextPath = pathForRoute(nextRoute);
	if (window.location.pathname !== nextPath) {
		window.history.pushState({}, "", nextPath);
	}
	route = nextRoute;
	if (route === "playground") layoutEditors();
	if (route === "coverage") scrollPageToTop();
	if (
		route === "playground" &&
		initialized &&
		(!cache.rustCode || !cache.compiled)
	)
		schedulePipeline(0);
}

let vmState: VmState = State.INITIALIZING;
let vmOverlayVisible = false;
let consoleLines: ConsoleLine[] = [];
let vmStartRequested = false;

let storedRatio = parseFloat(localStorage.getItem("gors:heightRatio") ?? "");
let editorsFlex =
	isNaN(storedRatio) || storedRatio <= 0
		? 1.35
		: Math.min(Math.max(storedRatio, 0.7), 2.6);
let editorPaneHeight: number | null = null;
let consolePaneHeight: number | null = null;
const MIN_EDITOR_HEIGHT = 220;
const MIN_CONSOLE_HEIGHT = 110;

let goEditor: monaco.editor.IStandaloneCodeEditor | null = null;
let rustEditor: monaco.editor.IStandaloneCodeEditor | null = null;

let editorsEl: HTMLDivElement;
let consoleSectionEl: HTMLDivElement;
let playgroundContentEl: HTMLDivElement;
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
let prevRoute: AppRoute = route;
$: if (route !== prevRoute) {
	prevRoute = route;
	if (route === "playground") layoutEditors();
}
$: pipelineBusy = activePipelines > 0;
$: runDisabled = pipelineBusy || !cache.compiled || !cache.jobId;
$: compileStatus =
	vmState === State.RUNNING
		? "Running"
		: cache.compiled
			? "Compiled"
			: cache.rustCode
				? route === "playground"
					? "Rust output ready"
					: "Rust ready"
				: pipelineBusy
					? "Transpiling"
					: "Waiting for changes";

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
	if (!initialized) return;
	pipelineDebounceTimer = setTimeout(() => {
		pipelineDebounceTimer = null;
		runPipeline();
	}, delay);
}

function setRustValue(value: string) {
	rustExpectedValue = value;
	rustEditor?.getModel()?.setValue(value);
}

function availablePaneHeight(): number | null {
	if (!playgroundContentEl) return null;
	const handle = playgroundContentEl.querySelector(".resize-handle");
	const handleRect = handle?.getBoundingClientRect();
	const handleStyles = handle ? getComputedStyle(handle) : null;
	const handleOuterHeight =
		(handleRect?.height ?? 0) +
		(Number.parseFloat(handleStyles?.marginTop ?? "0") || 0) +
		(Number.parseFloat(handleStyles?.marginBottom ?? "0") || 0);
	const styles = getComputedStyle(playgroundContentEl);
	const rowGap = Number.parseFloat(styles.rowGap || styles.gap || "0") || 0;
	return Math.max(
		MIN_EDITOR_HEIGHT + MIN_CONSOLE_HEIGHT,
		playgroundContentEl.clientHeight - handleOuterHeight - rowGap * 2,
	);
}

function setPaneHeights(editorHeight: number, totalHeight?: number) {
	const total =
		totalHeight ??
		availablePaneHeight() ??
		editorHeight + (consolePaneHeight ?? MIN_CONSOLE_HEIGHT);
	const clampedEditorHeight = Math.min(
		total - MIN_CONSOLE_HEIGHT,
		Math.max(MIN_EDITOR_HEIGHT, editorHeight),
	);
	editorPaneHeight = clampedEditorHeight;
	consolePaneHeight = total - clampedEditorHeight;
	if (editorsEl) {
		editorsEl.style.flex = "none";
		editorsEl.style.height = `${editorPaneHeight}px`;
	}
	if (consoleSectionEl) {
		consoleSectionEl.style.flex = "none";
		consoleSectionEl.style.height = `${consolePaneHeight}px`;
	}
	tick().then(layoutEditors);
}

function initializePaneHeights() {
	if (editorPaneHeight !== null && consolePaneHeight !== null) return;
	const total = availablePaneHeight();
	if (!total) return;
	const editorHeight = (total * editorsFlex) / (editorsFlex + 1);
	setPaneHeights(editorHeight, total);
}

function resetRustOutput() {
	setRustValue("");
	sourceMap = null;
	clearRustHighlight();
	const rustModel = rustEditor?.getModel();
	if (rustModel) monaco.editor.setModelMarkers(rustModel, "rustc", []);
}

async function waitForVM() {
	startVM();
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
	if (activePipelines > 0) {
		queuedPipeline = true;
		return;
	}
	activePipelines++;
	try {
		conClear();
		const rustCode = await doTranspile();
		if (!rustCode) return;
		await tick();
		if (route === "playground") await doCompile(rustCode);
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
	cache = { ...cache, jobId: null, compiled: false };
	resetRustOutput();
	if (activePipelines > 0) {
		go2rust.cancelActive("compiler input changed");
	}
	schedulePipeline();
}

async function handleRun() {
	cancelScheduledPipeline();
	if (!cache.jobId || !cache.compiled || activePipelines > 0) return;
	activePipelines++;
	try {
		conClear();
		await doRun(cache.jobId);
	} finally {
		activePipelines--;
		if (queuedPipeline) {
			queuedPipeline = false;
			schedulePipeline(0);
		}
	}
}

// Resize
function onResizePointerDown(e: PointerEvent) {
	e.preventDefault();
	if (!editorsEl || !consoleSectionEl || !playgroundContentEl) return;
	const startY = e.clientY;
	const startEH = editorsEl.offsetHeight;
	const total =
		availablePaneHeight() ?? startEH + consoleSectionEl.offsetHeight;
	setPaneHeights(startEH, total);
	document.body.style.cursor = "row-resize";
	document.body.style.userSelect = "none";
	function onMove(ev: PointerEvent) {
		setPaneHeights(startEH + ev.clientY - startY, total);
	}
	function onUp() {
		document.removeEventListener("pointermove", onMove);
		document.removeEventListener("pointerup", onUp);
		document.body.style.cursor = "";
		document.body.style.userSelect = "";
		const eh = editorPaneHeight ?? editorsEl.offsetHeight;
		const ch = consolePaneHeight ?? consoleSectionEl.offsetHeight;
		editorsFlex = ch > 0 ? eh / ch : 1.618;
		localStorage.setItem("gors:heightRatio", editorsFlex.toString());
		layoutEditors();
	}
	document.addEventListener("pointermove", onMove);
	document.addEventListener("pointerup", onUp);
}

// VM terminal overlay
function openVmOverlay() {
	startVM();
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

function startVM() {
	if (vmStartRequested) return;
	vmStartRequested = true;
	runner.start().catch(() => {
		vmState = State.ERROR;
	});
}

let resizeObserver: ResizeObserver | null = null;
let removePopStateListener: (() => void) | null = null;

onMount(() => {
	const onPopState = () => {
		route = routeFromPath(window.location.pathname);
		if (route === "coverage") scrollPageToTop();
		if (
			route === "playground" &&
			initialized &&
			(!cache.rustCode || !cache.compiled)
		)
			schedulePipeline(0);
	};
	window.addEventListener("popstate", onPopState);
	removePopStateListener = () =>
		window.removeEventListener("popstate", onPopState);

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

	// Resize observer for terminal
	resizeObserver = new ResizeObserver(() => {
		if (route === "playground") {
			editorPaneHeight = null;
			consolePaneHeight = null;
			tick().then(initializePaneHeights);
		}
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

$: if (!goEditor) goEditorReady = false;
$: if (!rustEditor) rustEditorReady = false;
$: if (!goEditor || !rustEditor) initialized = false;

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
	if (route === "playground") goEditor.focus();
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
	initializePaneHeights();
	if (route === "playground") schedulePipeline(0);
}

let initialized = false;

onDestroy(() => {
	cancelScheduledPipeline();
	removePopStateListener?.();
	go2rust.dispose();
	resizeObserver?.disconnect();
	term?.dispose();
});
</script>

<svelte:window on:keydown={onKeydown} />

<main class="site-shell">
  <header class="site-header">
    <a class="brand" href="/" on:click={(event) => navigateTo("home", event)}>gors</a>
    <nav class="site-nav" aria-label="Primary navigation">
      <a href="/" class:active={route === "home"} on:click={(event) => navigateTo("home", event)}>Home</a>
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

  {#if route === "home"}
  <div class="home-route">
    <section class="hero">
      <div class="hero-copy">
        <p class="eyebrow">Go compiler frontend, Rust backend</p>
        <h1>gors</h1>
        <p class="hero-subtitle">
          gors is a Go-to-Rust compiler pipeline: it parses real Go source, resolves packages from a pinned Go SDK, lowers the AST into Rust, and prints normal Rust code.
        </p>
        <div class="hero-actions">
          <a href="/playground" class="primary-link" on:click={(event) => navigateTo("playground", event)}>Open playground</a>
        </div>
      </div>

      <div class="compiler-card" aria-label="Go to Rust compiler pipeline preview">
        <p class="compiler-card-kicker">Same path as the CLI</p>
        <h2>Go source moves through a real compiler pipeline.</h2>
        <div class="pipeline-flow" aria-hidden="true">
          <div class="flow-node go-node"><span>Go source</span></div>
          <span class="flow-arrow">-></span>
          <div class="flow-node"><span>Scanner</span></div>
          <span class="flow-arrow">-></span>
          <div class="flow-node"><span>Parser</span></div>
          <span class="flow-arrow">-></span>
          <div class="flow-node ast-node"><span>Go AST</span></div>
          <span class="flow-arrow">-></span>
          <div class="flow-node"><span>Lowering</span></div>
          <span class="flow-arrow">-></span>
          <div class="flow-node ast-node"><span>Rust AST</span></div>
          <span class="flow-arrow">-></span>
          <div class="flow-node"><span>Passes</span></div>
          <span class="flow-arrow">-></span>
          <div class="flow-node rust-node"><span>Rust source</span></div>
          <i class="flow-pulse pulse-one"></i>
          <i class="flow-pulse pulse-two"></i>
        </div>
        <div class="compiler-card-footer">
          <code>gors build main.go</code>
          <span>main.rs</span>
        </div>
      </div>
    </section>

    <section class="home-details" aria-label="Project details">
      <div>
        <p class="eyebrow">Less guesswork</p>
        <h2>See the Rust a Go program becomes</h2>
        <p>The playground keeps the generated Rust visible, so compiler behavior can be inspected directly instead of hidden behind a black-box run button.</p>
      </div>
      <div>
        <p class="eyebrow">Real inputs</p>
        <h2>Exercise ordinary Go packages</h2>
        <p>Stdlib packages are resolved from the pinned Go SDK as source files, which keeps progress tied to generic compiler support rather than custom package rewrites.</p>
      </div>
      <div>
        <p class="eyebrow">Proof surface</p>
        <h2>Coverage you can audit</h2>
        <p>Generated Rust behavior is checked against the pinned Go SDK, and the coverage report shows which packages and symbols are backed by integration tests.</p>
      </div>
    </section>

    <section class="info-grid" aria-label="About gors">
      <article>
        <h2>Real compiler path</h2>
        <p>Scanner, parser, AST lowering, Rust AST passes, pretty printing, and source-map lookup are shared with the CLI path.</p>
      </article>
      <article>
        <h2>Go stdlib as source</h2>
        <p>The embedded SDK is pinned from the repository version file, and stdlib packages are resolved as Go files instead of handwritten Rust shims.</p>
      </article>
      <article>
        <h2>Runnable checks</h2>
        <p>Integration tests compare generated Rust programs against the pinned Go SDK. <a href="/coverage" on:click={(event) => navigateTo("coverage", event)}>View coverage</a>.</p>
      </article>
    </section>

    <footer class="site-footer">
      <div>
        <p class="eyebrow">Try gors</p>
        <strong>Inspect the generated Rust path</strong>
        <span>Use the browser playground for small programs, or review coverage to see which stdlib symbols are exercised by integration tests.</span>
      </div>
      <div class="footer-actions">
        <a href="/playground" class="primary-link" on:click={(event) => navigateTo("playground", event)}>Open playground</a>
      </div>
    </footer>
  </div>
  {/if}

  {#if route === "playground"}
  <div class="editor-route">
    <section id="playground" class="playground-section">
      <div class="section-heading">
        <div>
          <p class="eyebrow">Live playground</p>
          <h2>Go in, Rust out, run it</h2>
        </div>
        <span class="compile-status" class:ready={cache.compiled || !!cache.rustCode} class:busy={pipelineBusy}>{compileStatus}</span>
      </div>

      <div class="content playground-content" bind:this={playgroundContentEl}>
        <div
          class="editors"
          bind:this={editorsEl}
          style:flex={editorPaneHeight === null ? editorsFlex : "none"}
          style:height={editorPaneHeight === null ? null : `${editorPaneHeight}px`}
        >
          <div class="editor-container go">
            <div class="editor-header">
              <div class="label"><span class="dot"></span><span>main.go</span></div>
              <div class="actions">
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
                <button class="action-button run-button" title="Run the compiled program in the Linux VM" on:click={handleRun} disabled={runDisabled}>
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
        <div class="resize-handle" on:pointerdown={onResizePointerDown}></div>

        <div
          class="console-section"
          bind:this={consoleSectionEl}
          style:flex={consolePaneHeight === null ? 1 : "none"}
          style:height={consolePaneHeight === null ? null : `${consolePaneHeight}px`}
        >
          <div class="console-header">
            <div class="console-left">
              <div class="console-title"><span class="dot"></span><span>Console</span></div>
            </div>
            <div class="console-right">
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
              <CopyButton getContent={getConsoleText} title="Copy console output" />
            </div>
          </div>
          <pre class="console-content">{#each consoleLines as line}<span class={line.type}>{@html formatConsoleLine(line)}</span>{'\n'}{/each}</pre>
        </div>
      </div>
    </section>

  </div>
  {/if}

  {#if route === "coverage"}
  <div class="coverage-route">
    <CoveragePage />
  </div>
  {/if}
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
  :global(*) {
    box-sizing: border-box;
  }

  :global(html),
  :global(body),
  :global(#app) {
    max-width: 100%;
    min-height: 100%;
    overflow-x: hidden;
  }

  :global(body) {
    margin: 0;
    padding: 0;
    background: #f5f7fb;
    color: #1f2328;
    font-family: system-ui, -apple-system, sans-serif;
  }

  :global(.source-map-highlight) {
    border-radius: 2px;
    background-color: rgba(9, 105, 218, 0.18);
  }

  :global(.monaco-editor .lines-content) {
    padding-left: 5px;
  }

  :global(.monaco-editor .editor-widget) {
    z-index: 50;
  }

  .site-shell {
    display: flex;
    max-width: 100%;
    min-height: 100vh;
    flex-direction: column;
    overflow-x: clip;
    padding-top: 51px;
  }

  .site-header {
    position: fixed;
    top: 0;
    right: 0;
    left: 0;
    z-index: 60;
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 10px 20px;
    border-bottom: 1px solid #d0d7de;
    background: rgba(255, 255, 255, 0.94);
    backdrop-filter: blur(12px);
  }

  .brand {
    color: #1f2328;
    font-size: 17px;
    font-weight: 760;
    text-decoration: none;
  }

  .brand:hover {
    color: #0969da;
  }

  .site-nav {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .site-nav a {
    min-height: 30px;
    padding: 6px 10px;
    border-radius: 6px;
    color: #57606a;
    font-size: 13px;
    font-weight: 650;
    text-decoration: none;
  }

  .site-nav a:hover,
  .site-nav a.active {
    background: #eaeef2;
    color: #0969da;
  }

  .github-link {
    display: inline-flex;
    min-height: 30px;
    align-items: center;
    gap: 6px;
    padding: 6px 10px;
    border-radius: 6px;
    color: #57606a;
    font-size: 13px;
    font-weight: 650;
    text-decoration: none;
  }

  .github-link svg {
    width: 16px;
    height: 16px;
    flex-shrink: 0;
    fill: currentColor;
  }

  .github-link:hover {
    background: #eaeef2;
    color: #0969da;
  }

  .spacer {
    flex: 1;
  }

  .hidden {
    display: none !important;
  }

  .home-route {
    flex: 1;
  }

  .editor-route {
    display: flex;
    flex: 1;
    height: calc(100vh - 51px);
    max-height: calc(100vh - 51px);
    min-height: calc(100vh - 51px);
    min-width: 0;
    flex-direction: column;
    overflow: hidden;
  }

  .coverage-route {
    display: flex;
    flex: 1;
    max-width: 100%;
    height: calc(100vh - 51px);
    max-height: calc(100vh - 51px);
    min-height: calc(100vh - 51px);
    min-width: 0;
    flex-direction: column;
    overflow-x: clip;
  }

  .hero {
    display: grid;
    min-height: 560px;
    grid-template-columns: minmax(0, 0.9fr) minmax(420px, 0.85fr);
    align-items: center;
    gap: 46px;
    padding: 58px 48px 44px;
    background:
      linear-gradient(135deg, rgba(45, 164, 78, 0.12), transparent 45%),
      linear-gradient(45deg, rgba(255, 200, 50, 0.18), transparent 38%),
      #f5f7fb;
  }

  .hero-copy {
    max-width: 760px;
  }

  .eyebrow {
    margin: 0 0 8px;
    color: #57606a;
    font-size: 12px;
    font-weight: 760;
    letter-spacing: 0;
    text-transform: uppercase;
  }

  .hero h1 {
    margin: 0;
    color: #1f2328;
    font-size: clamp(56px, 9vw, 112px);
    line-height: 0.92;
  }

  .hero-subtitle {
    max-width: 700px;
    margin: 18px 0 0;
    color: #424a53;
    font-size: 21px;
    line-height: 1.45;
  }

  .hero-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    margin-top: 28px;
  }

  .primary-link,
  .secondary-link {
    display: inline-flex;
    min-height: 38px;
    align-items: center;
    padding: 8px 13px;
    border-radius: 6px;
    font-size: 14px;
    font-weight: 700;
    text-decoration: none;
  }

  .primary-link {
    border: 1px solid #1f2328;
    background: #1f2328;
    color: #ffffff;
  }

  .primary-link:hover {
    background: #0969da;
    border-color: #0969da;
  }

  .secondary-link {
    border: 1px solid #d0d7de;
    background: #ffffff;
    color: #0969da;
  }

  .secondary-link:hover {
    border-color: #0969da;
  }

  .compiler-card {
    padding: 24px;
    overflow: hidden;
    border: 1px solid #30363d;
    border-radius: 8px;
    background:
      linear-gradient(135deg, rgba(0, 173, 216, 0.12), transparent 45%),
      #0d1117;
    box-shadow: 0 24px 70px rgba(31, 35, 40, 0.24);
  }

  .compiler-card-kicker {
    margin: 0;
    color: #8b949e;
    font-size: 12px;
    font-weight: 700;
    letter-spacing: 0;
    text-transform: uppercase;
  }

  .compiler-card h2 {
    max-width: 560px;
    margin: 10px 0 0;
    color: #f0f6fc;
    font-size: 28px;
    line-height: 1.18;
  }

  .pipeline-flow {
    position: relative;
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 28px;
    padding: 6px 0;
  }

  .pipeline-flow::before {
    position: absolute;
    top: 50%;
    right: 8px;
    left: 8px;
    height: 2px;
    background: linear-gradient(90deg, #00add8, #ffc832, #2da44e);
    opacity: 0.42;
    transform: translateY(-50%);
    content: "";
  }

  .flow-node {
    position: relative;
    z-index: 1;
    display: flex;
    min-height: 48px;
    align-items: center;
    justify-content: center;
    padding: 9px 10px;
    border: 1px solid #30363d;
    border-radius: 6px;
    background: rgba(22, 27, 34, 0.96);
    box-shadow: 0 10px 24px rgba(1, 4, 9, 0.24);
  }

  .flow-node span {
    color: #c9d1d9;
    font-size: 12px;
    font-weight: 700;
    text-align: center;
    white-space: nowrap;
  }

  .flow-arrow {
    position: relative;
    z-index: 1;
    color: #8b949e;
    font-family: "Fira Code Variable", "Fira Code", monospace;
    font-size: 13px;
    font-weight: 700;
    line-height: 1;
  }

  .go-node {
    border-color: rgba(0, 173, 216, 0.8);
  }

  .ast-node {
    border-color: rgba(255, 200, 50, 0.75);
  }

  .rust-node {
    border-color: rgba(45, 164, 78, 0.75);
  }

  .flow-pulse {
    position: absolute;
    top: 50%;
    z-index: 2;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: #f0f6fc;
    box-shadow:
      0 0 0 4px rgba(88, 166, 255, 0.18),
      0 0 18px rgba(88, 166, 255, 0.8);
    transform: translate(-50%, -50%);
    animation: flow-pulse 5.5s linear infinite;
  }

  .pulse-two {
    animation-delay: 2.75s;
  }

  .compiler-card-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-top: 24px;
    padding-top: 18px;
    border-top: 1px solid #30363d;
    color: #8b949e;
    font-size: 13px;
  }

  .compiler-card-footer code {
    color: #ffc832;
    font-family: "Fira Code Variable", "Fira Code", monospace;
  }

  .info-grid {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 1px;
    overflow: hidden;
    border-top: 1px solid #d0d7de;
    background: #d0d7de;
  }

  .info-grid article {
    min-height: 172px;
    padding: 24px;
    background: #ffffff;
  }

  .info-grid h2 {
    margin: 0;
    color: #1f2328;
    font-size: 18px;
  }

  .info-grid p {
    margin: 10px 0 0;
    color: #57606a;
    font-size: 14px;
    line-height: 1.5;
  }

  .info-grid a {
    color: #0969da;
    font-weight: 650;
    text-decoration: none;
  }

  .info-grid a:hover {
    text-decoration: underline;
  }

  .home-details {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 24px;
    padding: 40px 48px 48px;
    border-top: 1px solid #d0d7de;
    background: #ffffff;
  }

  .home-details div {
    min-width: 0;
  }

  .home-details h2 {
    margin: 0;
    color: #1f2328;
    font-size: 24px;
    line-height: 1.15;
  }

  .home-details p:last-child {
    margin: 12px 0 0;
    color: #57606a;
    font-size: 15px;
    line-height: 1.55;
  }

  .site-footer {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    gap: 24px;
    padding: 34px 48px;
    border-top: 1px solid #d0d7de;
    background: #f6f8fa;
  }

  .site-footer div {
    display: flex;
    min-width: 0;
    flex-direction: column;
    align-items: flex-start;
    gap: 6px;
  }

  .site-footer strong {
    color: #1f2328;
    font-size: 24px;
    line-height: 1.15;
  }

  .site-footer span {
    color: #57606a;
    font-size: 14px;
    line-height: 1.45;
  }

  .site-footer p {
    max-width: 720px;
    margin: 0;
    color: #424a53;
    font-size: 13px;
    line-height: 1.55;
  }

  .footer-actions {
    align-items: center;
    flex-direction: row;
    justify-content: flex-end;
  }

  .playground-section {
    display: flex;
    flex: 1;
    height: 100%;
    min-height: 0;
    flex-direction: column;
    overflow: hidden;
    padding: 18px 18px 16px;
    background: #11151c;
    color: #c9d1d9;
  }

  .section-heading {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 16px;
    max-width: 1680px;
    width: 100%;
    margin: 0 auto 12px;
  }

  .section-heading .eyebrow {
    color: #8b949e;
  }

  .section-heading h2 {
    margin: 0;
    color: #f0f6fc;
    font-size: 24px;
    line-height: 1.05;
  }

  .compile-status {
    min-height: 28px;
    padding: 6px 10px;
    border: 1px solid #30363d;
    border-radius: 6px;
    color: #8b949e;
    font-size: 12px;
    font-weight: 700;
  }

  .compile-status.ready {
    border-color: #2ea043;
    color: #3fb950;
  }

  .compile-status.busy {
    border-color: #1f6feb;
    color: #58a6ff;
  }

  .content {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    overflow: hidden;
  }

  .playground-content {
    width: 100%;
    max-width: 1680px;
    min-height: 0;
    overflow: hidden;
    margin: 0 auto;
  }

  .editors {
    display: flex;
    flex-shrink: 0;
    min-height: 220px;
    gap: 12px;
  }

  .editor-container {
    display: flex;
    min-width: 0;
    flex: 1;
    flex-direction: column;
  }

  .editor-header {
    display: flex;
    height: 36px;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
    border-radius: 8px 8px 0 0;
    background: #161b22;
    font-size: 12px;
    font-weight: 650;
  }

  .editor-header .label {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .editor-header .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }

  .go .dot {
    background: #00add8;
  }

  .rust .dot {
    background: #ffc832;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .editor-wrapper {
    position: relative;
    flex: 1;
    overflow: visible;
    border: 2px solid;
    border-top: none;
    border-radius: 0 0 8px 8px;
  }

  .go .editor-wrapper {
    border-color: #00add8;
  }

  .rust .editor-wrapper {
    border-color: #ffc832;
  }

  .action-button {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 8px;
    border: 1px solid #30363d;
    border-radius: 4px;
    background: transparent;
    color: #8b949e;
    cursor: pointer;
    font: inherit;
    font-size: 11px;
    transition: all 0.15s ease;
  }

  .action-button:hover:not(:disabled) {
    border-color: #8b949e;
    background: #21262d;
    color: #c9d1d9;
  }

  .action-button:disabled {
    cursor: not-allowed;
    opacity: 0.35;
  }

  .action-button svg {
    width: 12px;
    height: 12px;
    fill: currentColor;
  }

  .run-button:not(:disabled) {
    border-color: #2ea043;
    color: #3fb950;
  }

  .btn-spinner {
    width: 12px;
    height: 12px;
    flex-shrink: 0;
    border: 2px solid #30363d;
    border-top-color: #8b949e;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  .resize-handle {
    display: flex;
    height: 16px;
    flex-shrink: 0;
    align-items: center;
    justify-content: center;
    margin: 0;
    cursor: row-resize;
    touch-action: none;
    user-select: none;
  }

  .resize-handle::after {
    width: 72px;
    height: 4px;
    border-radius: 2px;
    background: #30363d;
    content: "";
    transition: background 0.15s;
  }

  .resize-handle:hover::after {
    background: #484f58;
  }

  .console-section {
    display: flex;
    flex-shrink: 0;
    min-height: 110px;
    flex: 1;
    flex-direction: column;
    overflow: hidden;
    border: 2px solid #30363d;
    border-radius: 8px;
  }

  .console-header {
    display: flex;
    height: 36px;
    flex-shrink: 0;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
    background: #161b22;
    font-size: 12px;
    font-weight: 650;
  }

  .console-left,
  .console-right,
  .console-title {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .console-title .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: #c9d1d9;
  }

  .console-content {
    flex: 1;
    margin: 10px;
    overflow-y: auto;
    background: #0d1117;
    color: #c9d1d9;
    font-family: "Fira Code Variable", "Fira Code", monospace;
    font-size: 13px;
    line-height: 1.5;
    white-space: pre-wrap;
    word-break: break-all;
  }

  .console-content :global(.cmd),
  .console-content :global(.out) {
    color: #c9d1d9;
  }

  .console-content :global(.err) {
    color: #f85149;
  }

  .console-content :global(a) {
    color: #58a6ff;
    text-decoration: none;
  }

  .console-content :global(a:hover) {
    text-decoration: underline;
  }

  .vm-status {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 5px 8px;
    border: 1px solid transparent;
    border-radius: 5px;
    cursor: pointer;
    font-family: inherit;
    font-size: 11px;
    font-weight: 650;
    transition: all 0.15s ease;
    user-select: none;
  }

  .vm-status:hover {
    border-color: #d0d7de;
    background: #f6f8fa;
  }

  .vm-dot {
    width: 6px;
    height: 6px;
    flex-shrink: 0;
    border-radius: 50%;
  }

  [data-state="initializing"] > .vm-dot,
  [data-state="downloading"] > .vm-dot,
  [data-state="booting"] > .vm-dot {
    background: #bf8700;
  }

  [data-state="booting"] > .vm-dot,
  [data-state="compiling"] > .vm-dot,
  [data-state="running"] > .vm-dot {
    animation: pulse 1.2s ease-in-out infinite;
  }

  [data-state="ready"] > .vm-dot {
    background: #1a7f37;
  }

  [data-state="compiling"] > .vm-dot,
  [data-state="running"] > .vm-dot {
    background: #0969da;
  }

  [data-state="error"] > .vm-dot {
    background: #cf222e;
  }

  [data-state="initializing"] > .vm-label,
  [data-state="downloading"] > .vm-label,
  [data-state="booting"] > .vm-label {
    color: #9a6700;
  }

  [data-state="ready"] > .vm-label {
    color: #1a7f37;
  }

  [data-state="compiling"] > .vm-label,
  [data-state="running"] > .vm-label {
    color: #0969da;
  }

  [data-state="error"] > .vm-label {
    color: #cf222e;
  }

  .vm-terminal-overlay {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: none;
    background: rgba(31, 35, 40, 0.55);
  }

  .vm-terminal-overlay.visible {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 48px;
  }

  .vm-terminal-panel {
    display: flex;
    width: 800px;
    height: 600px;
    flex-direction: column;
    overflow: hidden;
    border: 1px solid #30363d;
    border-radius: 12px;
    background: #0d1117;
  }

  .vm-terminal-header {
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    padding: 10px 12px;
    border-bottom: 1px solid #21262d;
    background: #161b22;
  }

  .vm-terminal-left {
    min-width: 0;
  }

  .vm-terminal-title {
    color: #c9d1d9;
    font-size: 13px;
    font-weight: 650;
    text-align: center;
  }

  .vm-terminal-right {
    display: flex;
    justify-content: flex-end;
  }

  .vm-terminal-close {
    padding: 4px 8px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: #8b949e;
    cursor: pointer;
    font-size: 16px;
  }

  .vm-terminal-close:hover {
    background: #21262d;
    color: #c9d1d9;
  }

  .vm-terminal-body {
    flex: 1;
    overflow: hidden;
    padding: 8px;
  }

  .vm-spinner-container {
    display: flex;
    flex: 1;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 16px;
  }

  .vm-spinner {
    width: 32px;
    height: 32px;
    border: 3px solid #30363d;
    border-top-color: #58a6ff;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  .vm-spinner-label {
    color: #8b949e;
    font-size: 13px;
  }

  @keyframes pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.35;
    }
  }

  @keyframes flow-pulse {
    0% {
      left: 4%;
      opacity: 0;
    }
    8% {
      opacity: 1;
    }
    92% {
      opacity: 1;
    }
    100% {
      left: 96%;
      opacity: 0;
    }
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (max-width: 980px) {
    .site-header {
      gap: 8px;
      padding: 10px 12px;
    }

    .spacer {
      display: none;
    }

    .site-nav {
      flex: 1;
      justify-content: center;
      min-width: 0;
    }

    .site-nav a,
    .github-link {
      padding: 6px 7px;
      font-size: 12px;
    }

    .github-link span {
      display: none;
    }

    .hero {
      grid-template-columns: 1fr;
      min-height: auto;
      padding: 36px 18px 28px;
    }

    .pipeline-flow {
      flex-wrap: wrap;
    }

    .flow-pulse {
      display: none;
    }

    .compiler-card-footer {
      align-items: flex-start;
      flex-direction: column;
    }

    .info-grid {
      grid-template-columns: 1fr;
    }

    .home-details {
      grid-template-columns: 1fr;
      padding: 28px 18px 32px;
    }

    .site-footer {
      grid-template-columns: 1fr;
      padding: 22px 18px;
    }

    .footer-actions {
      flex-direction: row;
    }

    .playground-section {
      min-height: 0;
      padding: 14px 10px;
    }

    .section-heading {
      align-items: flex-start;
      flex-direction: column;
    }

    .editors {
      flex-direction: column;
      gap: 10px;
    }

    .vm-terminal-overlay.visible {
      padding: 16px;
    }

    .vm-terminal-panel {
      width: 100%;
      height: min(680px, 90vh);
    }
  }
</style>
