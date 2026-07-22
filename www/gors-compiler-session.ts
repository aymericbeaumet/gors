import {
	loadGorsWasm,
	type GorsCompiler,
	type GorsWasm,
} from "./gors-wasm-loader";

type BindingsLoader = () => Promise<GorsWasm>;

/** Track which successful source revision is installed in the retained compiler. */
export class CompilerSourceRevision {
	#currentSuccessfulSource: string | null = null;

	canReuseCachedResult(source: string): boolean {
		return this.#currentSuccessfulSource === source;
	}

	beginCompile(): void {
		this.#currentSuccessfulSource = null;
	}

	completeCompile(source: string, success: boolean): void {
		this.#currentSuccessfulSource = success ? source : null;
	}
}

/** Create one lazy compiler owner for the lifetime of its worker module. */
export function createCompilerSessionLoader(
	loadBindings: BindingsLoader = loadGorsWasm,
): () => Promise<GorsCompiler> {
	let compilerPromise: Promise<GorsCompiler> | null = null;
	return () => {
		compilerPromise ??= loadBindings().then(
			({ GorsCompiler: Compiler }) => new Compiler(),
		);
		return compilerPromise;
	};
}
