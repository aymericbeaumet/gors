<script lang="ts">
import { onDestroy, onMount, tick } from "svelte";
import * as monaco from "monaco-editor";
import { Terminal } from "xterm";
import { FitAddon } from "@xterm/addon-fit";
import defaultPlaygroundSource from "../default-playground.go";
import {
	CompilerCancelledError,
	Go2RustCompiler,
	type CompileResult,
} from "../go2rust-compiler";
import type { CompilerPhase, CompilerPhaseTiming } from "../go2rust-protocol";
import {
	admitRuntimeDependency,
	type RuntimeDependency,
} from "../runtime-dependency";
import { RustRunner, State, type State as VmState } from "../rust-runner";
import {
	formatConsoleLine,
	type ConsoleLine,
	type ConsoleLineType,
} from "./console-format";
import { parseRustcErrors } from "./rustc-errors";
import type { SourceMapIndex } from "./source-map-index";
import CopyButton from "./CopyButton.svelte";
import MonacoEditor from "./MonacoEditor.svelte";
import VmTerminalOverlay from "./VmTerminalOverlay.svelte";
import "./styles/playground.css";

const ANSI_RE = /\x1b\[[0-9;]*m/g;
const PIPELINE_DEBOUNCE_MS = 350;
const DEFAULT_EDITOR_CONSOLE_RATIO = 1.61803398875;
const MIN_EDITOR_HEIGHT = 220;
const MIN_CONSOLE_HEIGHT = 110;

type PipelineStage = "idle" | "gors" | "rustc" | "main";

interface PipelineCache {
	goSource: string | null;
	rustCode: string | null;
	runtimeDependency: RuntimeDependency | null;
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

export let active: boolean;

let vmState: VmState = State.INITIALIZING;
let vmOverlayVisible = false;
let consoleLines: ConsoleLine[] = [];
let vmStartRequested = false;

let storedRatio = parseFloat(localStorage.getItem("gors:heightRatio") ?? "");
let editorsFlex =
	isNaN(storedRatio) || storedRatio <= 0
		? DEFAULT_EDITOR_CONSOLE_RATIO
		: Math.min(Math.max(storedRatio, 0.7), 2.6);
let editorPaneHeight: number | null = null;
let consolePaneHeight: number | null = null;

let goEditor: monaco.editor.IStandaloneCodeEditor | null = null;
let rustEditor: monaco.editor.IStandaloneCodeEditor | null = null;
let initialized = false;

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

let rustExpectedValue = "";
let activePipelines = 0;
let pipelineStage: PipelineStage = "idle";
let compilerPhase: CompilerPhase | null = null;

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
let wasActive = active;
$: if (active !== wasActive) {
	wasActive = active;
	if (active) layoutEditors();
}
$: pipelineBusy = activePipelines > 0;
$: runDisabled =
	pipelineBusy || !cache.rustCode || cache.runtimeDependency === null;
$: runButtonLabel =
	pipelineStage === "gors"
		? compilerPhase === "loading-wasm"
			? "loading gors"
			: compilerPhase === "indexing-source-map"
				? "source map"
				: compilerPhase === "queued"
					? "gors queued"
					: "gors"
		: pipelineStage === "rustc"
			? "rustc"
			: pipelineStage === "main"
				? "main"
				: "Run";
$: runButtonBusy = pipelineStage !== "idle";

function layoutEditors() {
	tick().then(() => {
		goEditor?.layout();
		rustEditor?.layout();
	});
}

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
	return consoleLines.map((line) => line.text).join("\n");
}

function formatDuration(durationMs: number): string {
	return durationMs < 1000
		? `${Math.round(durationMs)}ms`
		: `${(durationMs / 1000).toFixed(2)}s`;
}

function formatCompilerTimings(
	timings: readonly CompilerPhaseTiming[],
): string {
	return timings
		.filter(({ durationMs }) => durationMs >= 1)
		.map(({ phase, durationMs }) => `${phase} ${formatDuration(durationMs)}`)
		.join(", ");
}

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
				options: {
					className: "source-map-highlight",
					isWholeLine: false,
				},
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
				options: {
					className: "source-map-highlight",
					isWholeLine: false,
				},
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

let cache: PipelineCache = {
	goSource: null,
	rustCode: null,
	runtimeDependency: null,
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
		await new Promise<void>((resolve) => {
			const unsubscribe = runner.onStateChange((state) => {
				if (state === State.READY) {
					unsubscribe();
					resolve();
				}
			});
		});
	}
}

async function doTranspile() {
	if (!goEditor || !rustEditor) return null;
	const activeGoModel = goEditor.getModel();
	if (!activeGoModel) return null;
	const goCode = activeGoModel.getValue();
	if (cache.goSource === goCode && cache.rustCode !== null) {
		cache.runtimeDependency = admitRuntimeDependency(
			cache.runtimeDependency,
			"playground transpile cache reuse",
		);
		return cache.rustCode;
	}

	cache = {
		goSource: null,
		rustCode: null,
		runtimeDependency: null,
		jobId: null,
		compiled: false,
	};
	++pipelineGeneration;
	const goModel = goEditor.getModel();
	const rustModel = rustEditor.getModel();
	if (!goModel || !rustModel) return null;

	setRustValue("");
	monaco.editor.setModelMarkers(goModel, "gors", []);
	monaco.editor.setModelMarkers(rustModel, "rustc", []);
	sourceMap = null;

	conCmd("$ gors build -o main.rs main.go");
	pipelineStage = "gors";
	compilerPhase = "queued";
	await tick();
	const generation = pipelineGeneration;
	let goResult: CompileResult;
	try {
		goResult = await go2rust.compile(goCode, (status) => {
			if (generation === pipelineGeneration) compilerPhase = status.phase;
		});
	} catch (error) {
		pipelineStage = "idle";
		compilerPhase = null;
		if (error instanceof CompilerCancelledError) return null;
		conErr(error instanceof Error ? error.message : String(error));
		return null;
	}
	pipelineStage = "idle";
	compilerPhase = null;
	if (
		generation !== pipelineGeneration ||
		goCode !== activeGoModel.getValue()
	) {
		return null;
	}

	if (!goResult.success) {
		const error = goResult.error;
		const location = error.line > 0 ? `:${error.line}:${error.column}` : "";
		conErr(`main.go${location}: ${error.kind}: ${error.message}`);
		if (error.line > 0) {
			const lines = goCode.split("\n");
			let endColumn = error.endColumn || error.column + 1;
			if (error.line <= lines.length)
				endColumn = Math.min(endColumn, lines[error.line - 1].length + 1);
			monaco.editor.setModelMarkers(goModel, "gors", [
				{
					severity: monaco.MarkerSeverity.Error,
					message: error.message,
					startLineNumber: error.line,
					startColumn: error.column,
					endLineNumber: error.line,
					endColumn,
					source: "gors",
					code: error.kind,
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
	const timingSummary = formatCompilerTimings(goResult.timings);
	if (timingSummary) conOut(`gors phases: ${timingSummary}`);
	setRustValue(goResult.rustCode);
	sourceMap = goResult.sourceMap;
	const runtimeDependency = admitRuntimeDependency(
		goResult.runtimeDependency,
		"playground transpile cache insertion",
	);
	cache = {
		goSource: goCode,
		rustCode: goResult.rustCode,
		runtimeDependency,
		jobId: null,
		compiled: false,
	};
	return goResult.rustCode;
}

async function doCompile(
	rustCode: string,
	runtimeDependency: RuntimeDependency,
) {
	if (!rustEditor) return null;
	if (cache.compiled && cache.jobId) return cache.jobId;

	const generation = pipelineGeneration;
	pipelineStage = "rustc";
	conCmd("$ rustc -o main main.rs");
	await waitForVM();
	if (generation !== pipelineGeneration) {
		pipelineStage = "idle";
		return null;
	}

	const startedAt = performance.now();
	const result = await runner.compile(rustCode, runtimeDependency);
	if (generation !== pipelineGeneration || result.cancelled) {
		pipelineStage = "idle";
		return null;
	}
	if (typeof result.jobId !== "string") {
		pipelineStage = "idle";
		return null;
	}

	const rustModel = rustEditor.getModel();
	if (!rustModel) {
		pipelineStage = "idle";
		return null;
	}
	monaco.editor.setModelMarkers(rustModel, "rustc", []);

	if (!result.compile.success) {
		conErr(result.compile.stderr);
		monaco.editor.setModelMarkers(
			rustModel,
			"rustc",
			parseRustcErrors(result.compile.stderr, monaco.MarkerSeverity),
		);
		pipelineStage = "idle";
		return null;
	}

	conOut(`rustc finished in ${formatDuration(performance.now() - startedAt)}`);
	pipelineStage = "idle";
	cache.compiled = true;
	cache.jobId = result.jobId;
	return result.jobId;
}

async function doRun(jobId: string) {
	const generation = pipelineGeneration;
	pipelineStage = "main";
	conCmd("$ ./main");
	const startedAt = performance.now();
	const result = await runner.runJob(jobId);
	if (generation !== pipelineGeneration || result.cancelled || !result.run) {
		pipelineStage = "idle";
		return;
	}

	conOut(result.run.stdout);
	conErr(result.run.stderr);
	if (result.run.exitCode !== 0 && !result.run.stderr) {
		conErr(`program exited with code ${result.run.exitCode}`);
	}
	conOut(`run finished in ${formatDuration(performance.now() - startedAt)}`);
	pipelineStage = "idle";
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
	} finally {
		pipelineStage = "idle";
		activePipelines--;
		if (queuedPipeline) {
			queuedPipeline = false;
			schedulePipeline(0);
		}
	}
}

function onGoChanged() {
	pipelineGeneration++;
	compilerPhase = null;
	cache = {
		goSource: null,
		rustCode: null,
		runtimeDependency: null,
		jobId: null,
		compiled: false,
	};
	resetRustOutput();
	if (activePipelines > 0) {
		go2rust.cancelActive("compiler input changed");
	}
	schedulePipeline();
}

async function handleRun() {
	cancelScheduledPipeline();
	if (!cache.rustCode || !cache.runtimeDependency || activePipelines > 0)
		return;
	activePipelines++;
	try {
		const jobId = await doCompile(cache.rustCode, cache.runtimeDependency);
		if (jobId) await doRun(jobId);
	} finally {
		pipelineStage = "idle";
		activePipelines--;
		if (queuedPipeline) {
			queuedPipeline = false;
			schedulePipeline(0);
		}
	}
}

function onResizePointerDown(event: PointerEvent) {
	event.preventDefault();
	if (!editorsEl || !consoleSectionEl || !playgroundContentEl) return;
	const startY = event.clientY;
	const startEditorHeight = editorsEl.offsetHeight;
	const total =
		availablePaneHeight() ?? startEditorHeight + consoleSectionEl.offsetHeight;
	setPaneHeights(startEditorHeight, total);
	document.body.style.cursor = "row-resize";
	document.body.style.userSelect = "none";
	function onMove(moveEvent: PointerEvent) {
		setPaneHeights(startEditorHeight + moveEvent.clientY - startY, total);
	}
	function onUp() {
		document.removeEventListener("pointermove", onMove);
		document.removeEventListener("pointerup", onUp);
		document.body.style.cursor = "";
		document.body.style.userSelect = "";
		const editorHeight = editorPaneHeight ?? editorsEl.offsetHeight;
		const consoleHeight = consolePaneHeight ?? consoleSectionEl.offsetHeight;
		editorsFlex =
			consoleHeight > 0
				? editorHeight / consoleHeight
				: DEFAULT_EDITOR_CONSOLE_RATIO;
		localStorage.setItem("gors:heightRatio", editorsFlex.toString());
		layoutEditors();
	}
	document.addEventListener("pointermove", onMove);
	document.addEventListener("pointerup", onUp);
}

function openVmOverlay() {
	startVM();
	vmOverlayVisible = true;
}

function closeVmOverlay() {
	vmOverlayVisible = false;
}

function startVM() {
	if (vmStartRequested) return;
	vmStartRequested = true;
	runner.start().catch(() => {
		vmState = State.ERROR;
	});
}

let resizeObserver: ResizeObserver | null = null;

onMount(() => {
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

	runner.onStateChange((state) => {
		vmState = state;
	});

	resizeObserver = new ResizeObserver(() => {
		if (active) {
			editorPaneHeight = null;
			consolePaneHeight = null;
			tick().then(initializePaneHeights);
		}
		if (vmOverlayVisible) fitAddon.fit();
	});
	resizeObserver.observe(vmTerminalEl);

	monaco.languages.registerHoverProvider("rust", {
		provideHover(model, position) {
			const markers = monaco.editor.getModelMarkers({ resource: model.uri });
			for (const marker of markers) {
				if (
					position.lineNumber >= marker.startLineNumber &&
					position.lineNumber <= marker.endLineNumber &&
					position.column >= marker.startColumn &&
					position.column <= marker.endColumn
				) {
					return {
						range: new monaco.Range(
							marker.startLineNumber,
							marker.startColumn,
							marker.endLineNumber,
							marker.endColumn,
						),
						contents: [
							{
								value: `**${marker.source}(${marker.code})**: ${marker.message}`,
							},
						],
					};
				}
			}
			return null;
		},
	});
});

let goEditorReady = false;
let rustEditorReady = false;

$: if (!goEditor) goEditorReady = false;
$: if (!rustEditor) rustEditorReady = false;
$: if (!goEditor || !rustEditor) initialized = false;

$: if (goEditor && !goEditorReady) {
	goEditorReady = true;
	goEditor.onMouseMove((event: monaco.editor.IEditorMouseEvent) => {
		if (event.target.position)
			highlightFromGo(
				event.target.position.lineNumber,
				event.target.position.column,
			);
	});
	goEditor.onMouseLeave(() => clearRustHighlight());
}

$: if (rustEditor && !rustEditorReady) {
	rustEditorReady = true;
	rustEditor.onMouseMove((event: monaco.editor.IEditorMouseEvent) => {
		if (event.target.position)
			highlightFromRust(
				event.target.position.lineNumber,
				event.target.position.column,
			);
	});
	rustEditor.onMouseLeave(() => clearGoHighlight());

	rustEditor.getModel()?.onDidChangeContent(() => {
		const model = rustEditor?.getModel();
		if (!model) return;
		const current = model.getValue();
		if (current !== rustExpectedValue) {
			const markers = monaco.editor.getModelMarkers({ resource: model.uri });
			model.setValue(rustExpectedValue);
			monaco.editor.setModelMarkers(model, "rustc", markers);
		}
	});
}

$: if (goEditor && rustEditor && !initialized) {
	initialized = true;
	if (active) goEditor.focus();
	goEditor.getModel()?.setValue(defaultPlaygroundSource.trimEnd());
	goEditor.setPosition({ lineNumber: 6, column: 2 });
	initializePaneHeights();
	if (active) schedulePipeline(0);
}

onDestroy(() => {
	cancelScheduledPipeline();
	go2rust.dispose();
	resizeObserver?.disconnect();
	term?.dispose();
});
</script>

{#if active}
  <div class="editor-route">
    <section id="playground" class="playground-section">
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
                <button type="button" class="action-button run-button" data-compiler-phase={compilerPhase ?? "idle"} title="Run the compiled program in the Linux VM" on:click={handleRun} disabled={runDisabled}>
                  {#if runButtonBusy}
                    <span class="btn-spinner"></span>
                  {:else}
                    <svg viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg">
                      <path d="M8 5v14l11-7z"/>
                    </svg>
                  {/if}
                  <span>{runButtonLabel}</span>
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

<VmTerminalOverlay
  visible={vmOverlayVisible}
  started={vmStarted}
  title={vmTitle}
  bind:terminalElement={vmTerminalEl}
  close={closeVmOverlay}
/>
