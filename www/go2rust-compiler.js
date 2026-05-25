const MAX_CACHE_ENTRIES = 32;

function extractRustTokenAt(lines, line, col) {
	const lineText = lines[line];
	if (!lineText || col >= lineText.length) return null;

	const start = lineText[col];
	if (!start) return null;

	if (
		col + 1 < lineText.length &&
		start === "/" &&
		(lineText[col + 1] === "/" || lineText[col + 1] === "*")
	) {
		if (lineText[col + 1] === "/") return lineText.slice(col);
	}

	if (/[A-Za-z_]/.test(start)) {
		let end = col;
		while (end < lineText.length && /[A-Za-z0-9_]/.test(lineText[end])) {
			end++;
		}
		if (lineText[end] === "!") end++;
		return lineText.slice(col, end);
	}

	return start;
}

function bestMapping(mappings, column, mappingColumnIndex) {
	if (!mappings?.length) return null;

	let best = null;
	let bestDistance = Number.POSITIVE_INFINITY;
	let bestIsBeforeCursor = false;

	for (const mapping of mappings) {
		const mappingColumn = mapping[mappingColumnIndex];
		const distance = Math.abs(column - mappingColumn);
		const isBeforeCursor = mappingColumn <= column;
		const dominated =
			(!bestIsBeforeCursor && isBeforeCursor) ||
			(bestIsBeforeCursor === isBeforeCursor && distance < bestDistance);

		if (dominated) {
			best = mapping;
			bestDistance = distance;
			bestIsBeforeCursor = isBeforeCursor;
		}
	}

	return best;
}

class SourceMapIndex {
	constructor(sourceMap, rustCode) {
		this.success = sourceMap?.success === true;
		this.rustLines = rustCode.split("\n");
		this.byGoLine = new Map();
		this.byOutputLine = new Map();

		for (const mapping of sourceMap?.mappings || []) {
			const [outputLine, , goLine] = mapping;
			if (!this.byGoLine.has(goLine)) this.byGoLine.set(goLine, []);
			this.byGoLine.get(goLine).push(mapping);
			if (!this.byOutputLine.has(outputLine))
				this.byOutputLine.set(outputLine, []);
			this.byOutputLine.get(outputLine).push(mapping);
		}
	}

	go_to_output(goLine, goColumn) {
		const line = Math.max(0, goLine - 1);
		const column = Math.max(0, goColumn - 1);
		const mapping = bestMapping(this.byGoLine.get(line), column, 3);
		if (!mapping) return [];

		const [outputLine, outputColumn] = mapping;
		const token = extractRustTokenAt(this.rustLines, outputLine, outputColumn);
		return [
			outputLine + 1,
			outputColumn + 1,
			outputLine + 1,
			outputColumn + (token?.length || 1) + 1,
		];
	}

	output_to_go(outputLine, outputColumn) {
		const line = Math.max(0, outputLine - 1);
		const column = Math.max(0, outputColumn - 1);
		const mapping = bestMapping(this.byOutputLine.get(line), column, 1);
		if (!mapping) return [];

		const [, , goLine, goColumn, name] = mapping;
		return [
			goLine + 1,
			goColumn + 1,
			goLine + 1,
			goColumn + (name?.length || 1) + 1,
		];
	}
}

export class Go2RustCompiler {
	constructor() {
		this.cache = new Map();
		this.worker = null;
		this.nextRequestId = 1;
		this.pending = new Map();
	}

	dispose() {
		if (this.worker) {
			this.worker.terminate();
			this.worker = null;
		}
		for (const { reject } of this.pending.values()) {
			reject(new Error("compiler worker disposed"));
		}
		this.pending.clear();
	}

	_worker() {
		if (!this.worker) {
			this.worker = new Worker(
				new URL("./go2rust-worker.js", import.meta.url),
				{
					type: "module",
				},
			);
			this.worker.onmessage = ({ data }) => this._handleWorkerMessage(data);
			this.worker.onerror = (event) => {
				for (const { reject } of this.pending.values()) {
					reject(new Error(event.message || "compiler worker error"));
				}
				this.pending.clear();
				this.worker?.terminate();
				this.worker = null;
			};
		}
		return this.worker;
	}

	_handleWorkerMessage(data) {
		const pending = this.pending.get(data?.id);
		if (!pending) return;
		this.pending.delete(data.id);

		if (!data.ok) {
			pending.reject(new Error(data.error || "compiler worker failed"));
			return;
		}

		const result = this._hydrateResult(data.result);
		this._touchCache(pending.goSource, result);
		pending.resolve(result);
	}

	_hydrateResult(result) {
		if (!result?.success) return result;
		return {
			...result,
			sourceMap: new SourceMapIndex(result.sourceMap, result.rustCode),
		};
	}

	_touchCache(goSource, result) {
		this.cache.delete(goSource);
		this.cache.set(goSource, result);
		if (this.cache.size > MAX_CACHE_ENTRIES) {
			const oldest = this.cache.keys().next().value;
			this.cache.delete(oldest);
		}
	}

	compile(goSource) {
		if (this.cache.has(goSource)) {
			const cached = this.cache.get(goSource);
			this._touchCache(goSource, cached);
			return Promise.resolve(cached);
		}

		const id = this.nextRequestId++;
		const promise = new Promise((resolve, reject) => {
			this.pending.set(id, { goSource, resolve, reject });
		});
		this._worker().postMessage({ type: "compile", id, goSource });
		return promise;
	}
}
