<script>
import { onMount, onDestroy, tick } from "svelte";
import * as monaco from "monaco-editor";
import { Terminal } from "xterm";
import { FitAddon } from "@xterm/addon-fit";
import { Go2RustCompiler } from "../go2rust-compiler.js";
import { RustRunner, State } from "../rust-runner.js";
import MonacoEditor from "./MonacoEditor.svelte";
import CopyButton from "./CopyButton.svelte";

const ANSI_RE = /\x1b\[[0-9;]*m/g;

const STATE_TITLES = {
	[State.INITIALIZING]: "VM initializing...",
	[State.DOWNLOADING]: "VM downloading...",
	[State.BOOTING]: "VM booting...",
	[State.READY]: "VM ready",
	[State.COMPILING]: "VM busy",
	[State.RUNNING]: "VM busy",
	[State.ERROR]: "VM error",
};

const MODES = [
	{ value: "autorun", label: "Transpile + Compile + Run" },
	{ value: "autocompile", label: "Transpile + Compile" },
	{ value: "autotranspile", label: "Transpile" },
	{ value: "manual", label: "Noop" },
];
const PIPELINE_DEBOUNCE_MS = 350;

let runMode = localStorage.getItem("gors:runMode") || "autorun";
let vmState = State.INITIALIZING;
let vmOverlayVisible = false;
let consoleLines = [];

let storedRatio = parseFloat(localStorage.getItem("gors:heightRatio"));
let editorsFlex = isNaN(storedRatio) || storedRatio <= 0 ? 1.618 : storedRatio;

let goEditor = null;
let rustEditor = null;

let editorsEl;
let consoleSectionEl;
let vmTerminalEl;

const go2rust = new Go2RustCompiler();
const runner = new RustRunner();
let pipelineGeneration = 0;
let pipelineDebounceTimer = null;
let queuedPipeline = false;

let sourceMap = null;
let goDecorations = [];
let rustDecorations = [];

// Read-only enforcement for rust editor
let rustExpectedValue = "";

let transpiling = false;
let activePipelines = 0;

// xterm
let term;
let fitAddon;

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
$: pipelineBusy = activePipelines > 0;
$: transpileDisabled = runMode !== "manual" || pipelineBusy;
$: compileDisabled =
	runMode === "autocompile" || runMode === "autorun" || pipelineBusy;
$: runDisabled = runMode === "autorun" || pipelineBusy;

// Console helpers
function conClear() {
	consoleLines = [];
}
function conCmd(text) {
	consoleLines = [...consoleLines, { type: "cmd", text }];
}
function conOut(text) {
	if (text) consoleLines = [...consoleLines, { type: "out", text }];
}
function conErr(text) {
	if (!text) return;
	const clean = text.replace(ANSI_RE, "");
	consoleLines = [...consoleLines, { type: "err", text: clean }];
}
function getConsoleText() {
	return consoleLines.map((l) => l.text).join("\n");
}

function escapeHtml(s) {
	return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

function linkifyRustErrors(html) {
	return html.replace(
		/rustc --explain (E\d{4})/g,
		'<a href="https://doc.rust-lang.org/error_codes/$1.html" target="_blank" rel="noopener">rustc --explain $1</a>',
	);
}

function formatConsoleLine(line) {
	if (line.type === "err") return linkifyRustErrors(escapeHtml(line.text));
	return escapeHtml(line.text);
}

// Rustc error parsing
function parseRustcErrors(text) {
	const markers = [];
	const clean = text.replace(ANSI_RE, "");
	const re =
		/^(error|warning)(?:\[([A-Z]\d+)\])?: (.+)\n\s*--> [^:]+:(\d+):(\d+)/gm;
	let m;
	while ((m = re.exec(clean)) !== null) {
		const severity =
			m[1] === "warning"
				? monaco.MarkerSeverity.Warning
				: monaco.MarkerSeverity.Error;
		const code = m[2] || "";
		const message = m[3];
		const line = parseInt(m[4], 10);
		const col = parseInt(m[5], 10);
		const after = clean.substring(
			m.index + m[0].length,
			m.index + m[0].length + 500,
		);
		const ul = after.match(/^\s*\|?\s*(\^+)/m);
		const endCol = ul ? col + ul[1].length : col + 1;
		markers.push({
			severity,
			message: code ? `${code}: ${message}` : message,
			startLineNumber: line,
			startColumn: col,
			endLineNumber: line,
			endColumn: endCol,
			source: "rustc",
			code,
		});
	}
	return markers;
}

// Source map highlighting
function highlightFromGo(line, column) {
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

function highlightFromRust(line, column) {
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
let cache = { goSource: null, rustCode: null, jobId: null, compiled: false };

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

function setRustValue(v) {
	rustExpectedValue = v;
	if (rustEditor) rustEditor.getModel().setValue(v);
}

async function waitForVM() {
	if (
		runner.state !== State.READY &&
		runner.state !== State.COMPILING &&
		runner.state !== State.RUNNING
	) {
		await new Promise((resolve) => {
			const unsub = runner.onStateChange((state) => {
				if (state === State.READY) {
					unsub();
					resolve();
				}
			});
		});
	}
}

async function doTranspile() {
	const goCode = goEditor.getModel().getValue();
	if (cache.goSource === goCode && cache.rustCode !== null)
		return cache.rustCode;

	cache = { goSource: null, rustCode: null, jobId: null, compiled: false };
	++pipelineGeneration;
	const goModel = goEditor.getModel();
	const rustModel = rustEditor.getModel();

	setRustValue("");
	monaco.editor.setModelMarkers(goModel, "gors", []);
	monaco.editor.setModelMarkers(rustModel, "rustc", []);
	sourceMap = null;

	conCmd("$ gors build -o main.rs main.go");
	transpiling = true;
	await tick();
	const goResult = go2rust.compile(goCode);
	transpiling = false;

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

async function doCompile(rustCode) {
	if (cache.compiled && cache.jobId) return cache.jobId;

	const gen = pipelineGeneration;
	await waitForVM();
	if (gen !== pipelineGeneration) return null;

	conCmd("$ rustc -o main main.rs");
	const result = await runner.compile(rustCode);
	if (gen !== pipelineGeneration) return null;
	if (result.cancelled) return null;

	const rustModel = rustEditor.getModel();
	monaco.editor.setModelMarkers(rustModel, "rustc", []);

	if (!result.compile.success) {
		conErr(result.compile.stderr);
		monaco.editor.setModelMarkers(
			rustModel,
			"rustc",
			parseRustcErrors(result.compile.stderr),
		);
		return null;
	}

	cache.compiled = true;
	cache.jobId = result.jobId;
	return result.jobId;
}

async function doRun(jobId) {
	const gen = pipelineGeneration;
	conCmd("$ ./main");
	const result = await runner.runJob(jobId);
	if (gen !== pipelineGeneration) return;
	if (result.cancelled) return;

	conOut(result.run.stdout);
	conErr(result.run.stderr);
	if (result.run.exitCode !== 0 && !result.run.stderr) {
		conErr(`program exited with code ${result.run.exitCode}`);
	}
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
		if (queuedPipeline && runMode !== "manual") {
			queuedPipeline = false;
			schedulePipeline(0);
		}
	}
}

function onGoChanged() {
	pipelineGeneration++;
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
function onResizeMousedown(e) {
	e.preventDefault();
	const startY = e.clientY;
	const startEH = editorsEl.offsetHeight;
	const startCH = consoleSectionEl.offsetHeight;
	const total = startEH + startCH;
	editorsEl.style.flex = "none";
	consoleSectionEl.style.flex = "none";
	editorsEl.style.height = startEH + "px";
	consoleSectionEl.style.height = startCH + "px";
	function onMove(ev) {
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

function onOverlayClick(e) {
	if (e.target === e.currentTarget) closeVmOverlay();
}

function onKeydown(e) {
	if (e.key === "Escape" && vmOverlayVisible) closeVmOverlay();
}

let resizeObserver;

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
	term.onData((data) => {
		if (runner._emulator) runner._emulator.serial0_send(data);
	});

	let serialByteQueue = [];
	let serialFlushTimer = null;
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
	goEditor.onMouseMove((e) => {
		if (e.target.position)
			highlightFromGo(e.target.position.lineNumber, e.target.position.column);
	});
	goEditor.onMouseLeave(() => clearRustHighlight());
}

$: if (rustEditor && !rustEditorReady) {
	rustEditorReady = true;
	rustEditor.onMouseMove((e) => {
		if (e.target.position)
			highlightFromRust(e.target.position.lineNumber, e.target.position.column);
	});
	rustEditor.onMouseLeave(() => clearGoHighlight());

	// Read-only enforcement
	rustEditor.getModel().onDidChangeContent(() => {
		const current = rustEditor.getModel().getValue();
		if (current !== rustExpectedValue) {
			const markers = monaco.editor.getModelMarkers({
				resource: rustEditor.getModel().uri,
			});
			rustEditor.getModel().setValue(rustExpectedValue);
			monaco.editor.setModelMarkers(rustEditor.getModel(), "rustc", markers);
		}
	});
}

$: if (goEditor && rustEditor && !initialized) {
	initialized = true;
	goEditor.focus();
	goEditor
		.getModel()
		.setValue(
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
	if (resizeObserver) resizeObserver.disconnect();
	if (term) term.dispose();
});
</script>

<svelte:window on:keydown={onKeydown} />

<main>
  <header>
    <h1><a href="https://github.com/aymericbeaumet/gors" target="_blank" rel="noopener">gors</a></h1>
    <p class="subtitle">Go toolchain written in Rust (parser, compiler, sandbox)</p>
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

  <div class="content">
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
            <CopyButton getContent={() => goEditor?.getModel().getValue()} title="Copy Go code" />
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
            <CopyButton getContent={() => rustEditor?.getModel().getValue()} title="Copy Rust code" />
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
</style>
