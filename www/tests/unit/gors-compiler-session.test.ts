import { describe, expect, it, vi } from "vitest";
import {
	CompilerSourceRevision,
	createCompilerSessionLoader,
} from "../../gors-compiler-session";
import type {
	GorsBuildResult,
	GorsCompiler,
	GorsWasm,
} from "../../gors-wasm-loader";

vi.mock("../../gors-wasm-loader", () => ({ loadGorsWasm: vi.fn() }));

describe("createCompilerSessionLoader", () => {
	it("constructs one retained compiler across concurrent and later edits", async () => {
		const construct = vi.fn();
		class TestCompiler implements GorsCompiler {
			constructor() {
				construct();
			}

			build_rust(_input: string): GorsBuildResult {
				throw new Error("not needed by this ownership test");
			}

			free(): void {}
		}
		const bindings: GorsWasm = {
			GorsCompiler: TestCompiler,
		};
		const loadBindings = vi.fn(async () => bindings);
		const loadCompiler = createCompilerSessionLoader(loadBindings);

		const [first, concurrent] = await Promise.all([
			loadCompiler(),
			loadCompiler(),
		]);
		const later = await loadCompiler();

		expect(first).toBe(concurrent);
		expect(first).toBe(later);
		expect(loadBindings).toHaveBeenCalledOnce();
		expect(construct).toHaveBeenCalledOnce();
	});

	it("retries bindings and compiler construction after a transient failure", async () => {
		class TestCompiler implements GorsCompiler {
			build_rust(_input: string): GorsBuildResult {
				throw new Error("not needed by this ownership test");
			}

			free(): void {}
		}
		const bindings: GorsWasm = {
			GorsCompiler: TestCompiler,
		};
		const loadBindings = vi
			.fn<() => Promise<GorsWasm>>()
			.mockRejectedValueOnce(new Error("transient bindings failure"))
			.mockResolvedValue(bindings);
		const loadCompiler = createCompilerSessionLoader(loadBindings);

		await expect(loadCompiler()).rejects.toThrow("transient bindings failure");
		await expect(loadCompiler()).resolves.toBeInstanceOf(TestCompiler);
		expect(loadBindings).toHaveBeenCalledTimes(2);
	});
});

describe("CompilerSourceRevision", () => {
	it("rejects an older artifact until that source is reinstalled", () => {
		const revision = new CompilerSourceRevision();
		const first = "package main\nfunc main() {}\n";
		const second = "package main\nfunc main() { println(1) }\n";

		revision.beginCompile();
		revision.completeCompile(first, true);
		expect(revision.canReuseCachedResult(first)).toBe(true);

		revision.beginCompile();
		revision.completeCompile(second, true);
		expect(revision.canReuseCachedResult(first)).toBe(false);
		expect(revision.canReuseCachedResult(second)).toBe(true);

		revision.beginCompile();
		revision.completeCompile(first, true);
		expect(revision.canReuseCachedResult(first)).toBe(true);
	});

	it("disables cache hits after an unsuccessful or interrupted build", () => {
		const revision = new CompilerSourceRevision();
		const source = "package main\nfunc main() {}\n";

		revision.completeCompile(source, true);
		revision.beginCompile();
		expect(revision.canReuseCachedResult(source)).toBe(false);

		revision.completeCompile(source, false);
		expect(revision.canReuseCachedResult(source)).toBe(false);
	});
});
