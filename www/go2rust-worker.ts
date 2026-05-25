import { loadGorsWasm } from "./gors-wasm-loader";
import type { StructuredSourceMap } from "./src/source-map-index";
import type { BuildResult } from "./wasm/pkg/gors.js";

const MAX_CACHE_ENTRIES = 32;

interface CompileRequest {
	type: "compile";
	id: number;
	goSource: string;
}

interface CompilerError {
	message: string;
	kind: string;
	line: number;
	column: number;
	endColumn: number;
}

type WorkerCompileResult =
	| {
			success: true;
			rustCode: string;
			sourceMap: StructuredSourceMap;
			error: null;
	  }
	| {
			success: false;
			rustCode: "";
			sourceMap: null;
			error: CompilerError;
	  };

const worker = self as unknown as {
	onmessage: ((event: MessageEvent<CompileRequest>) => void) | null;
	postMessage(message: unknown): void;
};

const cache = new Map<string, WorkerCompileResult>();

function postStatus(id: number, phase: string): void {
	worker.postMessage({ id, type: "status", phase });
}

function touchCache(key: string, value: WorkerCompileResult): void {
	cache.delete(key);
	cache.set(key, value);
	if (cache.size > MAX_CACHE_ENTRIES) {
		const oldest = cache.keys().next().value;
		if (oldest !== undefined) cache.delete(oldest);
	}
}

function normalizeResult(result: BuildResult): WorkerCompileResult {
	try {
		if (result.success) {
			return {
				success: true,
				rustCode: result.output,
				sourceMap: {
					success: true,
					mappings: JSON.parse(result.get_mappings_json()),
				},
				error: null,
			};
		}

		return {
			success: false,
			rustCode: "",
			sourceMap: null,
			error: {
				message: result.error_message,
				kind:
					result.error_kind === "scanner"
						? "scanner error"
						: result.error_kind === "parser"
							? "syntax error"
							: "compile error",
				line: result.error_line,
				column: result.error_column,
				endColumn: result.error_end_column,
			},
		};
	} finally {
		result.free();
	}
}

worker.onmessage = async ({ data }) => {
	if (data?.type !== "compile") return;

	const { id, goSource } = data;
	const startedAt = performance.now();

	try {
		if (cache.has(goSource)) {
			const cached = cache.get(goSource);
			if (cached) {
				touchCache(goSource, cached);
				worker.postMessage({
					id,
					ok: true,
					result: cached,
					durationMs: performance.now() - startedAt,
					cacheHit: true,
				});
				return;
			}
		}

		postStatus(id, "wasm-loading");
		const gors = await loadGorsWasm();
		postStatus(id, "wasm-started");
		const result = normalizeResult(gors.build_rust(goSource));
		postStatus(id, "wasm-finished");
		touchCache(goSource, result);
		worker.postMessage({
			id,
			ok: true,
			result,
			durationMs: performance.now() - startedAt,
			cacheHit: false,
		});
	} catch (error) {
		worker.postMessage({
			id,
			ok: false,
			error: error instanceof Error ? error.message : String(error),
		});
	}
};
