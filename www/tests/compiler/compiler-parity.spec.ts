import { readFile, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import type { HarnessResult } from "./harness";

const parityMode = process.env.GORS_COMPILER_PARITY_MODE;
const parityBaselinePath = process.env.GORS_COMPILER_PARITY_PATH;
const POST_SEED_PACKAGE = "encoding/hex";

const PARITY_CASES = [
	{
		name: "predeclared-only",
		source: ["package main", "", "func main() {", "\tprintln(6 * 7)", "}"].join(
			"\n",
		),
	},
	{
		name: "independent-stdlib-packages",
		source: [
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
		].join("\n"),
	},
	{
		name: "post-seed-resolver-package",
		source: [
			"package main",
			"",
			'import "encoding/hex"',
			"",
			"func main() {",
			"\tprintln(hex.EncodedLen(3))",
			"}",
		].join("\n"),
	},
] as const;

interface ParityBaseline {
	schema: 1;
	cases: Array<{
		name: string;
		source: string;
		rustCode: string;
	}>;
}

async function compile(page: Page, source: string): Promise<HarnessResult> {
	return page.evaluate(
		(sourceText) => window.__gorsCompilerHarness.compile(sourceText),
		source,
	);
}

test("stable and threaded compiler outputs are byte-identical", async ({
	page,
}) => {
	test.skip(
		parityMode !== "write" && parityMode !== "compare",
		"only runs in the two-stage CI parity check",
	);
	if (!parityBaselinePath) {
		throw new Error("GORS_COMPILER_PARITY_PATH is required");
	}

	await page.goto("/");
	await expect(page.locator("html")).toHaveAttribute(
		"data-compiler-harness",
		"ready",
	);
	expect(await page.evaluate(() => window.crossOriginIsolated)).toBe(true);

	const seed = JSON.parse(
		await readFile(resolve("generated/resolver-cache-seed-v1.bin"), "utf8"),
	) as { entries: Array<{ importPath: string }> };
	expect(
		seed.entries.some(({ importPath }) => importPath === POST_SEED_PACKAGE),
	).toBe(false);

	const cases: ParityBaseline["cases"] = [];
	for (const parityCase of PARITY_CASES) {
		const result = await compile(page, parityCase.source);
		expect(result.success, parityCase.name).toBe(true);
		cases.push({
			name: parityCase.name,
			source: parityCase.source,
			rustCode: result.rustCode,
		});
	}

	const actual: ParityBaseline = { schema: 1, cases };
	if (parityMode === "write") {
		expect(process.env.GORS_WASM_THREADS).not.toBe("1");
		await writeFile(parityBaselinePath, `${JSON.stringify(actual)}\n`);
		return;
	}

	expect(process.env.GORS_WASM_THREADS).toBe("1");
	const expected = JSON.parse(
		await readFile(parityBaselinePath, "utf8"),
	) as ParityBaseline;
	expect(actual).toEqual(expected);
});
