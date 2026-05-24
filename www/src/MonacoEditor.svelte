<script>
import { onMount, onDestroy, createEventDispatcher } from "svelte";
import * as monaco from "monaco-editor";

export let language = "plaintext";
export let editor = null;

const dispatch = createEventDispatcher();
let container;
let disposable;

const options = {
	cursorSurroundingLines: 5,
	folding: true,
	fontSize: 13,
	fontFamily: "'Fira Code Variable', 'Fira Code', monospace",
	fontLigatures: true,
	glyphMargin: false,
	lineDecorationsWidth: 0,
	lineNumbers: "on",
	lineNumbersMinChars: 4,
	minimap: { enabled: false },
	occurrencesHighlight: "off",
	overviewRulerLanes: 0,
	renderFinalNewline: "off",
	renderIndentGuides: false,
	renderLineHighlight: "none",
	scrollBeyondLastLine: false,
	selectionHighlight: false,
	theme: "vs-dark",
	automaticLayout: true,
	lightbulb: { enabled: "off" },
	quickSuggestions: false,
	contextmenu: false,
	hover: { enabled: true, delay: 200 },
};

onMount(() => {
	editor = monaco.editor.create(container, { ...options, language });
	disposable = editor.getModel().onDidChangeContent(() => {
		dispatch("change", editor.getModel().getValue());
	});
});

onDestroy(() => {
	if (disposable) disposable.dispose();
	if (editor) editor.dispose();
	editor = null;
});
</script>

<div bind:this={container} class="editor"></div>

<style>
  .editor { width: 100%; height: 100%; }
</style>
