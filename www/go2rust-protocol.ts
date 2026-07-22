export type CompilerPhase =
	| "queued"
	| "loading-wasm"
	| "compiling"
	| "indexing-source-map"
	| "hydrating-source-map"
	| "cache-hit"
	| "complete";

export interface CompilerPhaseTiming {
	phase: Exclude<CompilerPhase, "complete">;
	durationMs: number;
}

export interface CompilerStatus {
	requestId: number;
	phase: CompilerPhase;
	elapsedMs: number;
}

export interface PackedSourceMap {
	success: boolean;
	/**
	 * Flat groups of [output line, output column, Go line, Go column].
	 * Lines and columns follow Source Map v3: zero-based lines and UTF-16
	 * code-unit columns, ready for conversion to Monaco's one-based positions.
	 *
	 * Keeping positions in a typed array lets the worker transfer them without
	 * structured-cloning an array for every source-map token.
	 */
	positions: Uint32Array;
	names: string[];
}

export interface CompilerError {
	message: string;
	kind: string;
	line: number;
	/** One-based UTF-16 code-unit column for Monaco. */
	column: number;
	/** One-based exclusive UTF-16 code-unit column for Monaco. */
	endColumn: number;
}

export interface WorkerSuccessResult {
	success: true;
	rustCode: string;
	sourceMap: PackedSourceMap;
	error: null;
}

export interface WorkerErrorResult {
	success: false;
	rustCode: "";
	sourceMap: null;
	error: CompilerError;
}

export type WorkerCompileResult = WorkerSuccessResult | WorkerErrorResult;

export interface CompileRequest {
	type: "compile";
	id: number;
	goSource: string;
	/** Browser-harness-only synchronous stall used to test worker preemption. */
	testSynchronousDelayMs?: number;
}

export interface CancelRequest {
	type: "cancel";
	ids: number[];
}

export type WorkerRequest = CompileRequest | CancelRequest;

export type WorkerResponse =
	| {
			type: "ready";
			workerId: string;
	  }
	| {
			id: number;
			type: "status";
			phase: CompilerPhase;
			elapsedMs: number;
	  }
	| {
			id: number;
			type: "cancelled";
			reason: string;
	  }
	| {
			id: number;
			type: "result";
			ok: true;
			result: WorkerCompileResult;
			workerDurationMs: number;
			timings: CompilerPhaseTiming[];
			cacheHit: boolean;
	  }
	| {
			id: number;
			type: "result";
			ok: false;
			error: string;
	  };
