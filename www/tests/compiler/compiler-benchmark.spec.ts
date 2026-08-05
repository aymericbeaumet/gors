import { expect, test, type Page } from "@playwright/test";
import type { CompilerPhaseTiming } from "../../go2rust-protocol";
import type { HarnessResult } from "./harness";

const benchmarkEnabled = process.env.GORS_COMPILER_BENCHMARK === "1";

const FIRST_SOURCE = [
	"package main",
	"",
	"func main() {",
	'\tprintln("first", 2 < 5, 6 * 7)',
	"}",
].join("\n");

const EDITED_SOURCE = [
	"package main",
	"",
	"func main() {",
	'\tprintln("edited", 5 > 2, 10 + 5)',
	"}",
].join("\n");

const ADDITIONAL_SOURCE = [
	"package main",
	"",
	"func main() {",
	"\tvalue := 1",
	"\tvalue = value + 2",
	"\tprintln(value)",
	"}",
].join("\n");

interface BenchmarkSample {
	totalMs: number;
	workerMs: number;
	phases: CompilerPhaseTiming[];
	cacheHit: boolean;
}

function sample(result: HarnessResult): BenchmarkSample {
	return {
		totalMs: result.durationMs,
		workerMs: result.workerDurationMs,
		phases: result.timings,
		cacheHit: result.cacheHit,
	};
}

async function compile(page: Page, source: string): Promise<HarnessResult> {
	return page.evaluate(
		(sourceText) => window.__gorsCompilerHarness.compile(sourceText),
		source,
	);
}

test("browser compiler benchmark", async ({ page }) => {
	test.skip(
		!benchmarkEnabled,
		"set GORS_COMPILER_BENCHMARK=1 to run browser timings",
	);

	await page.goto("/");
	await expect(page.locator("html")).toHaveAttribute(
		"data-compiler-harness",
		"ready",
	);
	const first = await compile(page, FIRST_SOURCE);
	expect(first.success).toBe(true);
	expect(first.cacheHit).toBe(false);
	expect(first.rustCode).toContain("first");
	expect(first.timings.map(({ phase }) => phase)).toEqual(
		expect.arrayContaining(["loading-wasm", "compiling"]),
	);

	const firstStats = await page.evaluate(() =>
		window.__gorsCompilerHarness.stats(),
	);
	expect(firstStats.workerStartCount).toBe(1);

	const edited = await compile(page, EDITED_SOURCE);
	expect(edited.success).toBe(true);
	expect(edited.cacheHit).toBe(false);
	expect(edited.rustCode).toContain("edited");
	expect(edited.rustCode).not.toBe(first.rustCode);

	const repeated = await compile(page, EDITED_SOURCE);
	expect(repeated.success).toBe(true);
	expect(repeated.cacheHit).toBe(true);
	expect(repeated.rustCode).toBe(edited.rustCode);
	expect(repeated.timings.map(({ phase }) => phase)).toContain("cache-hit");

	const additionalProgram = await compile(page, ADDITIONAL_SOURCE);
	expect(additionalProgram.success).toBe(true);
	expect(additionalProgram.cacheHit).toBe(false);
	expect(additionalProgram.timings.map(({ phase }) => phase)).toContain(
		"compiling",
	);

	const finalStats = await page.evaluate(() =>
		window.__gorsCompilerHarness.stats(),
	);
	expect(finalStats.workerStartCount).toBe(1);
	expect(finalStats.pendingRequestCount).toBe(0);

	const record = {
		schema: "gors-browser-compiler-benchmark-v3",
		runtime: {
			model: "dedicated-worker",
		},
		samples: {
			firstCompile: sample(first),
			editedCompile: sample(edited),
			outputCacheExactRepeat: sample(repeated),
			uncachedAdditionalProgram: sample(additionalProgram),
		},
	};
	process.stdout.write(`${JSON.stringify(record)}\n`);
});
