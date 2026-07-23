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
	expect(unsupported.error?.message).toContain("GORS2001");
	expect(unsupported.error?.message).toContain(
		"imports are not implemented by the HIR/MIR backend",
	);
	expect(unsupported.error?.line).toBe(1);

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
