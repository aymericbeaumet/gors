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

type AppRoute = "home" | "coverage";

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
	return pathname.replace(/\/+$/, "") === "/coverage" ? "coverage" : "home";
}

function pathForRoute(nextRoute: AppRoute): string {
	return nextRoute === "coverage" ? "/coverage" : "/";
}

function layoutPlayground() {
	tick().then(() => {
		goEditor?.layout();
		rustEditor?.layout();
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
	if (route === "home") layoutPlayground();
}

function scrollToPlayground(event: MouseEvent) {
	event.preventDefault();
	navigateTo("home");
	tick().then(() => {
		document
			.getElementById("playground")
			?.scrollIntoView({ behavior: "smooth", block: "start" });
	});
}

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
let prevRoute: AppRoute = route;
$: if (route !== prevRoute) {
	prevRoute = route;
	if (route === "home") layoutPlayground();
}
$: pipelineBusy = activePipelines > 0;
$: runDisabled = pipelineBusy || !cache.compiled || !cache.jobId;
$: compileStatus =
	vmState === State.RUNNING
		? "Running"
		: pipelineBusy
			? "Auto compiling"
			: cache.compiled
				? "Ready to run"
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
	if (activePipelines > 0) {
		queuedPipeline = true;
		return;
	}
	activePipelines++;
	try {
		conClear();
		const rustCode = await doTranspile();
		if (!rustCode) return;
		await doCompile(rustCode);
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
let removePopStateListener: (() => void) | null = null;

onMount(() => {
	const onPopState = () => {
		route = routeFromPath(window.location.pathname);
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
	if (route === "home") goEditor.focus();
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
	schedulePipeline(0);
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
    <p class="subtitle">Go to Rust transpilation, in your browser</p>
    <nav class="site-nav" aria-label="Primary navigation">
      <a href="/" class:active={route === "home"} on:click={(event) => navigateTo("home", event)}>Playground</a>
      <a href="/coverage" class:active={route === "coverage"} on:click={(event) => navigateTo("coverage", event)}>Coverage</a>
      <a href="https://github.com/aymericbeaumet/gors" target="_blank" rel="noopener">GitHub</a>
    </nav>
    <div class="spacer"></div>
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

  <div class="home-route" class:hidden={route !== "home"} aria-hidden={route !== "home"}>
    <section class="hero">
      <div class="hero-copy">
        <p class="eyebrow">Go compiler frontend, Rust backend</p>
        <h1>gors</h1>
        <p class="hero-subtitle">
          gors parses Go source, resolves real Go packages, lowers the program into Rust syntax, and runs the generated binary in a browser-hosted Linux VM.
        </p>
        <div class="hero-actions">
          <a href="#playground" class="primary-link" on:click={scrollToPlayground}>Try the playground</a>
          <a href="/coverage" class="secondary-link" on:click={(event) => navigateTo("coverage", event)}>View stdlib coverage</a>
        </div>
      </div>
      <div class="hero-visual" aria-label="gors compiler pipeline">
        <div class="pipeline-stage">
          <span>Go source</span>
          <code>fmt.Println("Hello")</code>
        </div>
        <div class="pipeline-stage">
          <span>Rust output</span>
          <code>println!("Hello");</code>
        </div>
        <div class="pipeline-stage">
          <span>Browser VM</span>
          <code>rustc -o main main.rs</code>
        </div>
      </div>
    </section>

    <section class="info-grid" aria-label="About gors">
      <article>
        <h2>Real compiler path</h2>
        <p>Scanner, parser, AST lowering, Rust AST passes, pretty printing, and source-map lookup all run through the same pipeline used by the CLI.</p>
      </article>
      <article>
        <h2>Hermetic Go stdlib</h2>
        <p>The embedded Go SDK is pinned from the repository version file, and stdlib packages are resolved as Go source rather than handwritten browser shims.</p>
      </article>
      <article>
        <h2>Executable feedback</h2>
        <p>The playground auto-transpiles and auto-compiles after edits. Running is explicit, so output reflects the compiled program you choose to execute.</p>
      </article>
    </section>

    <section id="playground" class="playground-section">
      <div class="section-heading">
        <div>
          <p class="eyebrow">Live playground</p>
          <h2>Go in, Rust out</h2>
        </div>
        <span class="compile-status" class:ready={cache.compiled} class:busy={pipelineBusy}>{compileStatus}</span>
      </div>

      <div class="content playground-content">
        <div class="editors" bind:this={editorsEl} style="flex: {editorsFlex}">
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
    </section>
  </div>

  <div class="coverage-route" class:hidden={route !== "coverage"} aria-hidden={route !== "coverage"}>
    <CoveragePage />
  </div>
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
    min-height: 100%;
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
    min-height: 100vh;
    flex-direction: column;
  }

  .site-header {
    position: sticky;
    top: 0;
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

  .subtitle {
    margin: 0;
    color: #57606a;
    font-size: 13px;
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

  .spacer {
    flex: 1;
  }

  .hidden {
    display: none !important;
  }

  .home-route {
    flex: 1;
  }

  .coverage-route {
    display: flex;
    flex: 1;
    min-height: calc(100vh - 51px);
    flex-direction: column;
  }

  .hero {
    display: grid;
    min-height: 520px;
    grid-template-columns: minmax(0, 0.95fr) minmax(420px, 1.05fr);
    align-items: center;
    gap: 48px;
    padding: 56px 48px 40px;
    background:
      linear-gradient(135deg, rgba(45, 164, 78, 0.12), transparent 42%),
      linear-gradient(45deg, rgba(255, 200, 50, 0.16), transparent 38%),
      #f5f7fb;
  }

  .hero-copy {
    max-width: 680px;
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
    max-width: 620px;
    margin: 18px 0 0;
    color: #424a53;
    font-size: 20px;
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

  .hero-visual {
    display: grid;
    gap: 12px;
    padding: 18px;
    border: 1px solid #30363d;
    border-radius: 8px;
    background: #0d1117;
    box-shadow: 0 18px 48px rgba(31, 35, 40, 0.22);
  }

  .pipeline-stage {
    display: grid;
    grid-template-columns: 116px minmax(0, 1fr);
    align-items: center;
    gap: 12px;
    padding: 13px;
    border: 1px solid #30363d;
    border-radius: 6px;
    background: #161b22;
  }

  .pipeline-stage span {
    color: #8b949e;
    font-size: 12px;
    font-weight: 700;
  }

  .pipeline-stage code {
    overflow: hidden;
    color: #c9d1d9;
    font-family: "Fira Code Variable", "Fira Code", monospace;
    font-size: 13px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .info-grid {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 1px;
    border-top: 1px solid #d0d7de;
    border-bottom: 1px solid #d0d7de;
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

  .playground-section {
    display: flex;
    min-height: 820px;
    flex-direction: column;
    padding: 28px 24px 24px;
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
    margin: 0 auto 16px;
  }

  .section-heading .eyebrow {
    color: #8b949e;
  }

  .section-heading h2 {
    margin: 0;
    color: #f0f6fc;
    font-size: 28px;
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
  }

  .playground-content {
    width: 100%;
    max-width: 1680px;
    margin: 0 auto;
  }

  .editors {
    display: flex;
    min-height: 300px;
    gap: 16px;
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
    height: 6px;
    flex-shrink: 0;
    align-items: center;
    justify-content: center;
    margin: 5px 0;
    cursor: row-resize;
    user-select: none;
  }

  .resize-handle::after {
    width: 40px;
    height: 3px;
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
    min-height: 220px;
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
    margin: 12px;
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

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (max-width: 980px) {
    .site-header {
      flex-wrap: wrap;
    }

    .subtitle {
      display: none;
    }

    .spacer {
      display: none;
    }

    .hero {
      min-height: auto;
      grid-template-columns: 1fr;
      gap: 28px;
      padding: 36px 18px 28px;
    }

    .hero-visual {
      order: -1;
    }

    .pipeline-stage {
      grid-template-columns: 1fr;
    }

    .info-grid {
      grid-template-columns: 1fr;
    }

    .playground-section {
      min-height: 980px;
      padding: 20px 14px;
    }

    .section-heading {
      align-items: flex-start;
      flex-direction: column;
    }

    .editors {
      flex-direction: column;
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
