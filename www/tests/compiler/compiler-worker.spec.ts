import { expect, test, type Page } from "@playwright/test";
import type { HarnessResult } from "./harness";

function sourceWith(value: number): string {
	return ["package main", "", "func main() {", `\tprintln(${value})`, "}"].join(
		"\n",
	);
}

async function compile(page: Page, source: string): Promise<HarnessResult> {
	return page.evaluate(
		(sourceText) => window.__gorsCompilerHarness.compile(sourceText),
		source,
	);
}

async function openHarness(page: Page): Promise<void> {
	await page.goto("/");
	await expect(page.locator("html")).toHaveAttribute(
		"data-compiler-harness",
		"ready",
	);
}

test("persistent compiler worker handles cold, cached, edited, and coalesced inputs", async ({
	page,
}) => {
	const requestedAssets: string[] = [];
	page.on("request", (request) => requestedAssets.push(request.url()));

	await openHarness(page);
	const cold = await compile(page, sourceWith(1001));
	expect(cold.success).toBe(true);
	expect(cold.cacheHit).toBe(false);
	expect(cold.runtimeDependency).toEqual({
		schemaVersion: 1,
		contractIdentity: expect.stringMatching(/^[0-9a-f]{64}$/),
		operationIds: [14, 16],
	});
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

	const unsupported = await compile(
		page,
		[
			"package main",
			"",
			'import "fmt"',
			"",
			"func main() {",
			'\tfmt.Println("legacy fallback forbidden")',
			"}",
		].join("\n"),
	);
	expect(unsupported.success).toBe(false);
	expect(unsupported.runtimeDependency).toBeNull();
	expect(unsupported.error?.kind).toBe("compile error");
	expect(unsupported.error?.message).toContain("GORS2004");
	expect(unsupported.error?.message).toContain(
		"no package catalog owns this canonical path",
	);
	expect(unsupported.error?.line).toBe(3);

	const firstStats = await page.evaluate(() =>
		window.__gorsCompilerHarness.stats(),
	);
	expect(firstStats.workerStartCount).toBe(1);
	expect(firstStats.workerId).not.toBeNull();

	const restoredAfterError = await compile(page, sourceWith(1001));
	expect(restoredAfterError.success).toBe(true);
	expect(restoredAfterError.cacheHit).toBe(false);
	expect(restoredAfterError.timings.map(({ phase }) => phase)).toContain(
		"compiling",
	);

	const repeated = await compile(page, sourceWith(1001));
	expect(repeated.success).toBe(true);
	expect(repeated.cacheHit).toBe(true);
	expect(repeated.runtimeDependency).toEqual(cold.runtimeDependency);
	expect(repeated.timings.map(({ phase }) => phase)).toContain("cache-hit");

	const edited = await compile(page, sourceWith(1002));
	expect(edited.success).toBe(true);
	expect(edited.cacheHit).toBe(false);
	expect(edited.rustCode).toContain("1002");

	const restoredOlderArtifact = await compile(page, sourceWith(1001));
	expect(restoredOlderArtifact.success).toBe(true);
	expect(restoredOlderArtifact.cacheHit).toBe(false);
	expect(restoredOlderArtifact.rustCode).toContain("1001");

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

	const finalStats = await page.evaluate(() =>
		window.__gorsCompilerHarness.stats(),
	);
	expect(finalStats.workerStartCount).toBe(2);
	expect(finalStats.workerPreemptCount).toBe(1);
	expect(finalStats.workerId).not.toBe(firstStats.workerId);
	expect(finalStats.pendingRequestCount).toBe(0);
	expect(
		requestedAssets.some((url) => /(?:v86|rootfs|seabios|vgabios)/i.test(url)),
	).toBe(false);
});

test("cancelled compilation cannot populate the surviving exact-output cache", async ({
	page,
}) => {
	await openHarness(page);
	const outcomes = await page.evaluate(
		({ source, delayMs }) =>
			window.__gorsCompilerHarness.cancelThenRetrySameSource(source, delayMs),
		{ source: sourceWith(4001), delayMs: 500 },
	);

	expect(outcomes[0].status).toBe("cancelled");
	expect(outcomes[1].status).toBe("fulfilled");
	expect(outcomes[1].result?.success).toBe(true);
	expect(outcomes[1].result?.cacheHit).toBe(false);
	expect(outcomes[1].result?.rustCode).toContain("4001");

	const stats = await page.evaluate(() => window.__gorsCompilerHarness.stats());
	expect(stats.workerStartCount).toBe(2);
	expect(stats.workerPreemptCount).toBe(1);
	expect(stats.pendingRequestCount).toBe(0);
});

test("new input supersedes a worker stuck in Wasm loading", async ({
	page,
}) => {
	await openHarness(page);
	const outcomes = await page.evaluate(
		({ active, latest }) =>
			window.__gorsCompilerHarness.supersedeDuringWasmLoad(
				active,
				latest,
				30_000,
			),
		{
			active: sourceWith(5001),
			latest: sourceWith(5002),
		},
	);

	expect(outcomes[0].status).toBe("cancelled");
	expect(outcomes[1].status).toBe("fulfilled");
	expect(outcomes[1].result?.success).toBe(true);
	expect(outcomes[1].result?.rustCode).toContain("5002");

	const stats = await page.evaluate(() => window.__gorsCompilerHarness.stats());
	expect(stats.workerStartCount).toBe(2);
	expect(stats.workerPreemptCount).toBe(1);
	expect(stats.pendingRequestCount).toBe(0);
});

test("liveness watchdog replaces a worker that stops making progress", async ({
	page,
}) => {
	await openHarness(page);
	const { outcome, retry, stats } = await page.evaluate((source) => {
		return window.__gorsCompilerHarness.watchdogStuckWasmLoad(
			source,
			30_000,
			1_000,
		);
	}, sourceWith(5501));

	expect(outcome.status).toBe("rejected");
	expect(outcome.message).toContain(
		"compiler worker made no progress during loading-wasm",
	);
	expect(retry.status).toBe("fulfilled");
	expect(retry.result?.success).toBe(true);
	expect(retry.result?.rustCode).toContain("5501");
	expect(stats.workerStartCount).toBe(2);
	expect(stats.workerPreemptCount).toBe(1);
	expect(stats.pendingRequestCount).toBe(0);
	expect(stats.activeRequestId).toBeNull();
});

test("transient Wasm loader failure is retried in the retained worker", async ({
	page,
}) => {
	let wasmRequestCount = 0;
	await page.route(/\.wasm(?:\?.*)?$/, async (route) => {
		wasmRequestCount++;
		if (wasmRequestCount === 1) {
			await route.fulfill({
				status: 503,
				contentType: "text/plain",
				body: "transient compiler asset failure",
			});
			return;
		}
		await route.continue();
	});
	await openHarness(page);

	const first = await page.evaluate((source) => {
		return window.__gorsCompilerHarness.compileSettled(source);
	}, sourceWith(6001));
	expect(first.status).toBe("rejected");
	expect(first.message).toContain("HTTP 503");

	const retry = await page.evaluate((source) => {
		return window.__gorsCompilerHarness.compileSettled(source);
	}, sourceWith(6001));
	expect(retry.status).toBe("fulfilled");
	expect(retry.result?.success).toBe(true);
	expect(retry.result?.cacheHit).toBe(false);
	expect(wasmRequestCount).toBe(2);

	const stats = await page.evaluate(() => window.__gorsCompilerHarness.stats());
	expect(stats.workerStartCount).toBe(1);
	expect(stats.workerPreemptCount).toBe(0);
	expect(stats.pendingRequestCount).toBe(0);
});
