import { expect, test } from "@playwright/test";
import goSpecReport from "../../../gors/tests/reports/go-spec-conformance.json";
import goStdlibReport from "../../../gors/tests/reports/go-stdlib-conformance.json";

function coverageMetric(tested: number, total: number): string {
	if (total === 0) return "0/0 (0%)";
	return `${tested}/${total} (${((tested / total) * 100).toFixed(1)}%)`;
}

test.skip("default hello world auto-compiles and runs manually", async ({
	page,
}) => {
	const pageErrors: string[] = [];
	const consoleErrors: string[] = [];

	page.on("pageerror", (error) => pageErrors.push(error.message));
	page.on("console", (message) => {
		if (message.type() === "error") consoleErrors.push(message.text());
	});

	await page.goto("/playground");

	const consoleOutput = page.locator(".console-content");
	await expect(page.getByRole("button", { name: "gors" })).toBeVisible();
	await expect(consoleOutput).toContainText("$ gors build", {
		timeout: 2 * 60 * 1000,
	});
	await expect(consoleOutput).toContainText("gors transpiled", {
		timeout: 8 * 60 * 1000,
	});
	await expect(consoleOutput).not.toContainText("$ rustc -o main main.rs");
	await expect(consoleOutput).not.toContainText("$ ./main", { timeout: 1000 });
	await page.getByRole("button", { name: "Run" }).click();
	await expect(consoleOutput).toContainText("$ rustc -o main main.rs", {
		timeout: 7 * 60 * 1000,
	});
	await expect(consoleOutput).toContainText("$ ./main", {
		timeout: 9 * 60 * 1000,
	});
	await expect(consoleOutput).toContainText("Hello, World!", {
		timeout: 10 * 60 * 1000,
	});
	await expect
		.poll(() =>
			consoleOutput.evaluate((node) => {
				const text = node.textContent ?? "";
				return (
					text.indexOf("$ gors build") <
						text.indexOf("$ rustc -o main main.rs") &&
					text.indexOf("$ rustc -o main main.rs") < text.indexOf("$ ./main")
				);
			}),
		)
		.toBe(true);

	await page.locator(".go .monaco-editor .view-lines").click();
	await page.keyboard.press("ControlOrMeta+A");
	await page.keyboard.type(
		[
			"package main",
			"",
			'import "fmt"',
			"",
			"func main() {",
			'\tfmt.Println("Changed")',
			"}",
		].join("\n"),
	);
	await expect(consoleOutput).toContainText("gors transpiled", {
		timeout: 8 * 60 * 1000,
	});
	await expect(consoleOutput).not.toContainText("$ rustc -o main main.rs");
	await expect(page.locator(".rust .monaco-editor")).not.toContainText(
		"Hello, World!",
	);
	await expect(consoleOutput).not.toContainText("$ ./main", { timeout: 1000 });
	await expect(consoleOutput).not.toContainText("waiting for VM");
	await expect(consoleOutput).not.toContainText("VM ready in");

	expect(pageErrors).toEqual([]);
	expect(consoleErrors).toEqual([]);
});

test.skip("conformance route shows stdlib package and symbol coverage", async ({
	page,
}) => {
	await page.goto("/conformance");

	await expect(
		page.getByRole("heading", { name: "Go Standard Library Conformance" }),
	).toBeVisible();
	await expect(
		page.getByText("Go Language Specification Conformance"),
	).toBeVisible();
	await expect(
		page.getByText(
			coverageMetric(
				goSpecReport.summary.passingGroupCount,
				goSpecReport.summary.groupCount,
			),
		),
	).toBeVisible();
	await expect(
		page.getByText(
			coverageMetric(
				goSpecReport.summary.passingCaseCount,
				goSpecReport.summary.caseCount,
			),
		),
	).toBeVisible();
	await expect(
		page.getByText("Slice expressions share the original backing array"),
	).toBeVisible();
	await expect(page.getByText("Uncovered").first()).toBeVisible();
	await expect(
		page.getByText(
			coverageMetric(
				goStdlibReport.summary.passingGroupCount,
				goStdlibReport.summary.groupCount,
			),
		),
	).toBeVisible();
	await expect(
		page.getByText(
			coverageMetric(
				goStdlibReport.summary.passingCaseCount,
				goStdlibReport.summary.caseCount,
			),
		),
	).toBeVisible();

	await expect(
		page.locator(".package-cell > code").filter({ hasText: /^fmt$/ }),
	).toHaveClass(/(^|\s)partial(\s|$)/);
	await expect(
		page.locator(".package-cell span").filter({ hasText: "13/29 covered" }),
	).toHaveClass(/(^|\s)partial(\s|$)/);
	await expect(page.locator(".package-cell .fixture-cell")).toHaveCount(0);
	await expect(page.getByText("Println", { exact: true })).toBeVisible();
	await expect(
		page.getByRole("link", { name: "fmt", exact: true }).first(),
	).toHaveAttribute(
		"href",
		"https://github.com/aymericbeaumet/gors/tree/master/gors/tests/fixtures/go_stdlib/fmt",
	);

	await expect(
		page.locator(".stdlib-symbol").filter({ hasText: "Header.FileInfo" }),
	).toHaveClass(/(^|\s)none(\s|$)/);
	await expect(
		page.locator(".stdlib-symbol").filter({ hasText: "FileInfoHeader" }),
	).toHaveClass(/(^|\s)tested(\s|$)/);

	await expect(
		page
			.locator(".package-cell > code")
			.filter({ hasText: /^container\/list$/ }),
	).toHaveClass(/(^|\s)none(\s|$)/);
	await expect(
		page.locator(".package-cell span").filter({ hasText: "0/20 covered" }),
	).toHaveClass(/(^|\s)none(\s|$)/);

	await expect(
		page.locator(".package-cell > code").filter({ hasText: /^structs$/ }),
	).toHaveClass(/(^|\s)tested(\s|$)/);
	await expect(
		page.locator(".package-cell span").filter({ hasText: "1/1 covered" }),
	).toHaveClass(/(^|\s)tested(\s|$)/);

	await expect
		.poll(() =>
			page.evaluate(
				"document.documentElement.scrollWidth <= document.documentElement.clientWidth",
			),
		)
		.toBe(true);
	await page.evaluate("window.scrollTo(0, document.body.scrollHeight)");
	await page.getByRole("link", { name: "gors" }).click();
	await page.getByRole("link", { name: "Learn more." }).click();
	await expect.poll(() => page.evaluate("window.scrollY")).toBe(0);
});

test("home page links to playground without rendering the console", async ({
	page,
}) => {
	await page.goto("/");

	await expect(page.getByRole("heading", { name: "gors" })).toBeVisible();
	await expect(
		page.getByRole("link", { name: "Playground", exact: true }),
	).toBeVisible();
	await expect(
		page.getByRole("link", { name: "Conformance", exact: true }),
	).toBeVisible();
	await expect(page.locator(".console-section")).toHaveCount(0);
	await expect(page.locator(".editor-route")).toHaveCount(0);
	await expect(page.locator(".site-footer")).toHaveCount(0);
	await expect(page.getByText("Go source to Rust output")).toHaveCount(0);

	await page.getByRole("link", { name: "Try in Playground" }).click();
	await expect(page).toHaveURL(/\/playground$/);
	await expect(page.locator(".console-section")).toBeVisible();
	await expect(page.locator(".site-footer")).toHaveCount(0);
});
