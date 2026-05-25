import * as gors from "./wasm/pkg/gors.js";

const MAX_CACHE_ENTRIES = 32;
const cache = new Map();

function touchCache(key, value) {
	cache.delete(key);
	cache.set(key, value);
	if (cache.size > MAX_CACHE_ENTRIES) {
		const oldest = cache.keys().next().value;
		cache.delete(oldest);
	}
}

function normalizeResult(result) {
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

self.onmessage = ({ data }) => {
	if (data?.type !== "compile") return;

	const { id, goSource } = data;
	const startedAt = performance.now();

	try {
		if (cache.has(goSource)) {
			const cached = cache.get(goSource);
			touchCache(goSource, cached);
			self.postMessage({
				id,
				ok: true,
				result: cached,
				durationMs: performance.now() - startedAt,
				cacheHit: true,
			});
			return;
		}

		const result = normalizeResult(gors.build_rust(goSource));
		touchCache(goSource, result);
		self.postMessage({
			id,
			ok: true,
			result,
			durationMs: performance.now() - startedAt,
			cacheHit: false,
		});
	} catch (error) {
		self.postMessage({
			id,
			ok: false,
			error: error instanceof Error ? error.message : String(error),
		});
	}
};
