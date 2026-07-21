import { expect, test, type Page } from "@playwright/test";
import type { HarnessResult } from "./harness";

function sourceWith(value: number): string {
	return ["package main", "", "func main() {", `\tprintln(${value})`, "}"].join(
		"\n",
	);
}

const CMP_SOURCE = [
	"package main",
	"",
	'import "cmp"',
	"",
	"func main() {",
	"\tprintln(cmp.Compare(2, 5))",
	"}",
].join("\n");

const THREADED_SOURCE = [
	"package main",
	"",
	"import (",
	'\t"cmp"',
	'\t"math/bits"',
	")",
	"",
	"func main() {",
	"\tprintln(cmp.Compare(2, 5))",
	"\tprintln(bits.OnesCount(7))",
	"}",
].join("\n");

async function compile(page: Page, source: string): Promise<HarnessResult> {
	return page.evaluate(
		(sourceText) => window.__gorsCompilerHarness.compile(sourceText),
		source,
	);
}

test("threaded runtime initializes its Rayon pool before compiling", async ({
	page,
}) => {
	test.skip(
		process.env.GORS_WASM_THREADS !== "1",
		"only runs against the opt-in threaded artifact",
	);

	await page.goto("/");
	await expect(page.locator("html")).toHaveAttribute(
		"data-compiler-harness",
		"ready",
	);
	expect(await page.evaluate(() => window.crossOriginIsolated)).toBe(true);

	const result = await compile(page, THREADED_SOURCE);
	expect(result.success).toBe(true);
	expect(result.persistentCache.restored).toBe(true);
	expect(result.persistentCache.importedEntries).toBeGreaterThan(0);
	expect(result.statuses.map(({ phase }) => phase)).toContain("loading-wasm");

	const before = await page.evaluate(() =>
		window.__gorsCompilerHarness.stats(),
	);
	const outcomes = await page.evaluate(
		({ active, latest }) =>
			window.__gorsCompilerHarness.supersedeWithoutPreemption(
				active,
				latest,
				250,
			),
		{ active: sourceWith(9001), latest: sourceWith(9002) },
	);
	expect(outcomes[0].status).toBe("cancelled");
	expect(outcomes[1].status).toBe("fulfilled");
	expect(outcomes[1].result?.rustCode).toContain("9002");
	const after = await page.evaluate(() => window.__gorsCompilerHarness.stats());
	expect(after.workerStartCount).toBe(before.workerStartCount);
	expect(after.workerPreemptCount).toBe(0);
	expect(after.workerId).toBe(before.workerId);
	expect(after.workerUsesThreads).toBe(true);
});

test("persistent compiler worker handles cold, cached, edited, and coalesced inputs", async ({
	page,
}) => {
	const requestedAssets: string[] = [];
	page.on("request", (request) => requestedAssets.push(request.url()));

	await page.goto("/");
	await expect(page.locator("html")).toHaveAttribute(
		"data-compiler-harness",
		"ready",
	);
	expect(await page.evaluate(() => window.crossOriginIsolated)).toBe(true);

	const cold = await compile(page, sourceWith(1001));
	expect(cold.success).toBe(true);
	expect(cold.cacheHit).toBe(false);
	expect(cold.persistentCache.restored).toBe(true);
	expect(cold.persistentCache.importedEntries).toBeGreaterThan(0);
	expect(cold.statuses.map(({ phase }) => phase)).toEqual(
		expect.arrayContaining([
			"queued",
			"loading-wasm",
			"compiling",
			"indexing-source-map",
			"complete",
		]),
	);
	expect(cold.timings.map(({ phase }) => phase)).toContain("compiling");

	const firstStats = await page.evaluate(() =>
		window.__gorsCompilerHarness.stats(),
	);
	expect(firstStats.workerStartCount).toBe(1);
	expect(firstStats.workerId).not.toBeNull();

	const repeated = await compile(page, sourceWith(1001));
	expect(repeated.success).toBe(true);
	expect(repeated.cacheHit).toBe(true);
	expect(repeated.timings.map(({ phase }) => phase)).toContain("cache-hit");

	const edited = await compile(page, sourceWith(1002));
	expect(edited.success).toBe(true);
	expect(edited.cacheHit).toBe(false);
	expect(edited.rustCode).toContain("1002");

	const rapidOutcomes = await page.evaluate(
		(sources) => {
			return window.__gorsCompilerHarness.compileLatest(sources);
		},
		[sourceWith(2001), sourceWith(2002), sourceWith(2003)],
	);
	expect(rapidOutcomes.slice(0, -1).map(({ status }) => status)).toEqual([
		"cancelled",
		"cancelled",
	]);
	const latest = rapidOutcomes.at(-1);
	expect(latest?.status).toBe("fulfilled");
	expect(latest?.result?.success).toBe(true);
	expect(latest?.result?.rustCode).toContain("2003");

	const activeEditOutcomes = await page.evaluate(
		({ active, latest }) =>
			window.__gorsCompilerHarness.supersedeAfterCancellationDelay(
				active,
				latest,
				350,
				700,
			),
		{
			active: sourceWith(3001),
			latest: sourceWith(3002),
		},
	);
	expect(activeEditOutcomes[0].status).toBe("cancelled");
	expect(activeEditOutcomes[1].status).toBe("fulfilled");
	expect(activeEditOutcomes[1].result?.rustCode).toContain("3002");

	const repeatedEditResult = await page.evaluate(
		({ active, middle, latest }) =>
			window.__gorsCompilerHarness.preemptWithPendingFlush(
				active,
				middle,
				latest,
				500,
			),
		{
			active: sourceWith(4001),
			middle: sourceWith(4002),
			latest: sourceWith(4003),
		},
	);
	expect(
		repeatedEditResult.requests.slice(0, -1).map(({ status }) => status),
	).toEqual(["cancelled", "cancelled"]);
	expect(repeatedEditResult.requests[2].status).toBe("fulfilled");
	expect(repeatedEditResult.requests[2].result?.rustCode).toContain("4003");
	expect(repeatedEditResult.flush).toEqual({
		status: "rejected",
		name: "CompilerCancelledError",
	});

	const finalStats = await page.evaluate(() =>
		window.__gorsCompilerHarness.stats(),
	);
	expect(finalStats.workerStartCount).toBe(3);
	expect(finalStats.workerPreemptCount).toBe(2);
	expect(finalStats.workerId).not.toBe(firstStats.workerId);
	expect(finalStats.workerUsesThreads).toBe(false);
	expect(finalStats.pendingRequestCount).toBe(0);
	expect(finalStats.pendingCacheFlushCount).toBe(0);
	expect(
		requestedAssets.some((url) => /(?:v86|rootfs|seabios|vgabios)/i.test(url)),
	).toBe(false);
	expect(
		requestedAssets.some((url) => /resolver-cache-seed-v1/i.test(url)),
	).toBe(true);

	const stdlibCompile = await compile(page, CMP_SOURCE);
	expect(stdlibCompile.success).toBe(true);
	const storedBytes = await page.evaluate(() =>
		window.__gorsCompilerHarness.flushPersistentCache(),
	);
	expect(storedBytes).toBeGreaterThan(0);

	const seedRequestsBeforeRestore = requestedAssets.filter((url) =>
		/resolver-cache-seed-v1/i.test(url),
	).length;
	await page.evaluate(() => window.__gorsCompilerHarness.restartWorker());
	const restored = await compile(page, CMP_SOURCE);
	expect(restored.success).toBe(true);
	expect(restored.cacheHit).toBe(false);
	expect(restored.persistentCache.restored).toBe(true);
	expect(restored.persistentCache.importedEntries).toBeGreaterThan(0);
	expect(restored.statuses.map(({ phase }) => phase)).toContain(
		"loading-cache",
	);
	expect(
		requestedAssets.filter((url) => /resolver-cache-seed-v1/i.test(url)).length,
	).toBe(seedRequestsBeforeRestore);

	await page.evaluate(async () => {
		window.__gorsCompilerHarness.restartWorker();
		await window.__gorsCompilerHarness.replacePersistentCache(
			JSON.stringify({
				schema: 1,
				goVersion: "wrong",
				stdlibVersion: "wrong",
				compilerFingerprint: "wrong",
				entries: [],
			}),
		);
	});
	const afterVersionMiss = await compile(page, sourceWith(4001));
	expect(afterVersionMiss.success).toBe(true);
	expect(afterVersionMiss.persistentCache.restored).toBe(true);
	expect(afterVersionMiss.persistentCache.importedEntries).toBeGreaterThan(0);
	expect(
		requestedAssets.filter((url) => /resolver-cache-seed-v1/i.test(url)).length,
	).toBeGreaterThan(seedRequestsBeforeRestore);
});
