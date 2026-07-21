import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import type {
	CompilerPhaseTiming,
	PersistentCacheInfo,
} from "../../go2rust-protocol";
import type { HarnessResult } from "./harness";

const benchmarkEnabled = process.env.GORS_COMPILER_BENCHMARK === "1";
const resolverSeedPattern = /resolver-cache-seed-v1/i;
const PARALLEL_EXPANSION_PACKAGES = [
	"encoding/hex",
	"hash/crc32",
	"hash/crc64",
	"unicode/utf16",
] as const;

const FIRST_SOURCE = [
	"package main",
	"",
	"import (",
	'\t"cmp"',
	'\t"fmt"',
	'\t"math/bits"',
	")",
	"",
	"func main() {",
	'\tfmt.Println("first", cmp.Compare(2, 5), bits.OnesCount(7))',
	"}",
].join("\n");

const EDITED_SOURCE = [
	"package main",
	"",
	"import (",
	'\t"cmp"',
	'\t"fmt"',
	'\t"math/bits"',
	")",
	"",
	"func main() {",
	'\tfmt.Println("edited", cmp.Compare(5, 2), bits.OnesCount(15))',
	"}",
].join("\n");

const PARALLEL_EXPANSION_SOURCE = [
	"package main",
	"",
	"import (",
	'\t"encoding/hex"',
	'\t"hash/crc32"',
	'\t"hash/crc64"',
	'\t"unicode/utf16"',
	")",
	"",
	"func main() {",
	'\tprintln(hex.EncodeToString([]byte("go")))',
	"\tprintln(crc32.Size, crc32.IEEE)",
	"\tprintln(crc64.Size, uint64(crc64.ISO))",
	"\tprintln(utf16.RuneLen('😀'))",
	"}",
].join("\n");

interface BenchmarkSample {
	totalMs: number;
	workerMs: number;
	phases: CompilerPhaseTiming[];
	cacheHit: boolean;
	persistentCache: PersistentCacheInfo;
}

function sample(result: HarnessResult): BenchmarkSample {
	return {
		totalMs: result.durationMs,
		workerMs: result.workerDurationMs,
		phases: result.timings,
		cacheHit: result.cacheHit,
		persistentCache: result.persistentCache,
	};
}

async function compile(page: Page, source: string): Promise<HarnessResult> {
	return page.evaluate(
		(sourceText) => window.__gorsCompilerHarness.compile(sourceText),
		source,
	);
}

async function deletePersistentResolverCache(page: Page): Promise<void> {
	await page.evaluate(
		() =>
			new Promise<void>((resolve, reject) => {
				const request = indexedDB.deleteDatabase("gors-compiler-cache");
				request.onsuccess = () => resolve();
				request.onerror = () =>
					reject(request.error ?? new Error("failed to clear IndexedDB"));
				request.onblocked = () =>
					reject(new Error("resolver cache IndexedDB deletion was blocked"));
			}),
	);
}

test("browser compiler benchmark", async ({ page }) => {
	test.skip(
		!benchmarkEnabled,
		"set GORS_COMPILER_BENCHMARK=1 to run browser timings",
	);

	const requestedAssets: string[] = [];
	page.on("request", (request) => requestedAssets.push(request.url()));

	await page.goto("/");
	await expect(page.locator("html")).toHaveAttribute(
		"data-compiler-harness",
		"ready",
	);
	await deletePersistentResolverCache(page);

	const first = await compile(page, FIRST_SOURCE);
	expect(first.success).toBe(true);
	expect(first.cacheHit).toBe(false);
	expect(first.rustCode).toContain("first");
	expect(first.timings.map(({ phase }) => phase)).toEqual(
		expect.arrayContaining(["loading-wasm", "loading-cache", "compiling"]),
	);

	const firstStats = await page.evaluate(() => ({
		stats: window.__gorsCompilerHarness.stats(),
		crossOriginIsolated: window.crossOriginIsolated,
		hardwareConcurrency: navigator.hardwareConcurrency || 2,
	}));
	expect(firstStats.stats.workerUsesThreads).not.toBeNull();
	expect(firstStats.stats.workerStartCount).toBe(1);
	expect(firstStats.crossOriginIsolated).toBe(true);

	const firstSeedRequestCount = requestedAssets.filter((url) =>
		resolverSeedPattern.test(url),
	).length;
	expect(firstSeedRequestCount).toBe(1);
	expect(first.persistentCache.restored).toBe(true);
	expect(first.persistentCache.importedEntries).toBeGreaterThan(0);

	const persistedBytes = await page.evaluate(() =>
		window.__gorsCompilerHarness.flushPersistentCache(),
	);
	expect(persistedBytes).toBeGreaterThan(0);
	await page.evaluate(() => window.__gorsCompilerHarness.restartWorker());

	const edited = await compile(page, EDITED_SOURCE);
	expect(edited.success).toBe(true);
	expect(edited.cacheHit).toBe(false);
	expect(edited.rustCode).toContain("edited");
	expect(edited.rustCode).not.toBe(first.rustCode);
	expect(edited.persistentCache.restored).toBe(true);
	expect(edited.persistentCache.importedEntries).toBeGreaterThan(0);
	expect(edited.statuses.map(({ phase }) => phase)).toContain("loading-cache");

	const seedRequestCountAfterRestore = requestedAssets.filter((url) =>
		resolverSeedPattern.test(url),
	).length;
	expect(seedRequestCountAfterRestore).toBe(firstSeedRequestCount);

	const repeated = await compile(page, EDITED_SOURCE);
	expect(repeated.success).toBe(true);
	expect(repeated.cacheHit).toBe(true);
	expect(repeated.rustCode).toBe(edited.rustCode);
	expect(repeated.timings.map(({ phase }) => phase)).toContain("cache-hit");

	const seed = JSON.parse(
		await readFile(resolve("generated/resolver-cache-seed-v1.bin"), "utf8"),
	) as { entries: Array<{ importPath: string }> };
	const seededPackages = new Set(
		seed.entries.map(({ importPath }) => importPath),
	);
	for (const importPath of PARALLEL_EXPANSION_PACKAGES) {
		expect(seededPackages.has(importPath), importPath).toBe(false);
	}
	const parallelExpansion = await compile(page, PARALLEL_EXPANSION_SOURCE);
	expect(parallelExpansion.success).toBe(true);
	expect(parallelExpansion.cacheHit).toBe(false);
	expect(parallelExpansion.persistentCache.restored).toBe(true);
	expect(parallelExpansion.timings.map(({ phase }) => phase)).toContain(
		"compiling",
	);

	const finalStats = await page.evaluate(() =>
		window.__gorsCompilerHarness.stats(),
	);
	expect(finalStats.workerUsesThreads).toBe(firstStats.stats.workerUsesThreads);
	expect(finalStats.workerStartCount).toBe(1);
	expect(finalStats.pendingRequestCount).toBe(0);
	expect(finalStats.pendingCacheFlushCount).toBe(0);

	const threaded = finalStats.workerUsesThreads === true;
	const record = {
		schema: "gors-browser-compiler-benchmark-v1",
		runtime: {
			threadMode: threaded ? "wasm-rayon" : "wasm-single-threaded",
			hardwareConcurrency: firstStats.hardwareConcurrency,
			controllerWorkerCount: 1,
			rayonWorkerCount: threaded
				? Math.max(1, Math.min(4, firstStats.hardwareConcurrency - 1))
				: 0,
			rayonWorkerCountSource: threaded
				? "inferred-from-verified-loader-contract"
				: "disabled",
		},
		cache: {
			bundledSeedRequested: firstSeedRequestCount === 1,
			bundledSeedRestored: first.persistentCache.restored,
			persistedBytes,
			persistentRestoreAvoidedSeedFetch:
				seedRequestCountAfterRestore === firstSeedRequestCount,
		},
		samples: {
			bundledSeedFirstCompile: sample(first),
			persistentRestoreEditedCompile: sample(edited),
			outputCacheExactRepeat: sample(repeated),
			uncachedParallelExpansion: sample(parallelExpansion),
		},
	};
	process.stdout.write(`${JSON.stringify(record)}\n`);
});
