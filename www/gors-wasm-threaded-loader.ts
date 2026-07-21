import type { GorsBuildResult, GorsWasm } from "./gors-wasm-loader";
import initWasm, * as bindings from "gors-wasm-threads-package";

type ThreadedBindings = GorsWasm & {
	compiler_thread_count(): number;
	initThreadPool(threadCount: number): Promise<void>;
};

const threadedBindings = bindings as unknown as ThreadedBindings;
let initPromise: Promise<GorsWasm> | null = null;
const MAX_COMPILER_RAYON_WORKERS = 4;

export const usesThreadedRuntime = true;

function requestedThreadCount(): number {
	const hardwareThreads = navigator.hardwareConcurrency || 2;
	// Compilation itself occupies this controller worker, so leave one logical
	// CPU for it and the UI while Rayon owns the remaining workers. Generated
	// package tasks share resolver caches, so a small bounded pool avoids
	// atomics contention on high-core-count browsers.
	return Math.max(1, Math.min(MAX_COMPILER_RAYON_WORKERS, hardwareThreads - 1));
}

function assertThreadSupport(): void {
	if (
		!globalThis.crossOriginIsolated ||
		typeof SharedArrayBuffer === "undefined"
	) {
		throw new Error(
			"gors threaded Wasm preview requires cross-origin isolation and SharedArrayBuffer",
		);
	}
}

export function loadGorsWasm(): Promise<GorsWasm> {
	initPromise ??= (async () => {
		assertThreadSupport();
		await initWasm();
		const threadCount = requestedThreadCount();
		await threadedBindings.initThreadPool(threadCount);
		const initializedThreads = threadedBindings.compiler_thread_count();
		if (initializedThreads !== threadCount) {
			throw new Error(
				`gors threaded Wasm initialized ${initializedThreads} Rayon workers; expected ${threadCount}`,
			);
		}
		return threadedBindings;
	})();
	return initPromise;
}

export type { GorsBuildResult, GorsWasm };
