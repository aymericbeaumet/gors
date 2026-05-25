import { expect, test } from "@playwright/test";

test("default hello world auto-compiles and runs manually", async ({
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

	await page.locator(".go .monaco-editor textarea").click();
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

test("conformance route shows stdlib package and symbol coverage", async ({
	page,
}) => {
	await page.goto("/conformance");

	await expect(
		page.getByRole("heading", { name: "Go standard library conformance" }),
	).toBeVisible();
	await expect(page.getByText("Go specification conformance")).toBeVisible();
	await expect(page.getByText("51/353 (14.4%)")).toBeVisible();
	await expect(page.getByText("344/12599 (2.7%)")).toBeVisible();

	await page.getByRole("searchbox").fill("fmt");
	await expect(
		page.locator(".package-cell code").filter({ hasText: /^fmt$/ }),
	).toHaveClass(/(^|\s)partial(\s|$)/);
	await expect(
		page.locator(".package-cell span").filter({ hasText: "13/31 tested" }),
	).toHaveClass(/(^|\s)partial(\s|$)/);
	await expect(page.getByText("Println", { exact: true })).toBeVisible();
	await expect(
		page.getByRole("link", { name: "gostdlib_fmt" }).first(),
	).toHaveAttribute(
		"href",
		"https://github.com/aymericbeaumet/gors/tree/master/tests/fixtures/go_programs/gostdlib_fmt",
	);

	await page.getByRole("searchbox").fill("archive/tar");
	await expect(
		page.locator(".symbol-token").filter({ hasText: "Format.String" }),
	).toHaveClass(/(^|\s)untested(\s|$)/);
	await expect(
		page.locator(".symbol-token").filter({ hasText: "FileInfoHeader" }),
	).toHaveClass(/(^|\s)tested(\s|$)/);

	await page.getByRole("searchbox").fill("container/list");
	await expect(
		page.locator(".package-cell code").filter({ hasText: /^container\/list$/ }),
	).toHaveClass(/(^|\s)none(\s|$)/);
	await expect(
		page.locator(".package-cell span").filter({ hasText: "0/20 tested" }),
	).toHaveClass(/(^|\s)none(\s|$)/);

	await page.getByRole("searchbox").fill("structs");
	await expect(
		page.locator(".package-cell code").filter({ hasText: /^structs$/ }),
	).toHaveClass(/(^|\s)tested(\s|$)/);
	await expect(
		page.locator(".package-cell span").filter({ hasText: "1/1 tested" }),
	).toHaveClass(/(^|\s)tested(\s|$)/);

	await expect
		.poll(() =>
			page.evaluate(
				"document.documentElement.scrollWidth <= document.documentElement.clientWidth",
			),
		)
		.toBe(true);
	await page.getByRole("searchbox").fill("");
	await page.getByRole("button", { name: "Yellow" }).click();
	await expect(page).toHaveURL(/color=yellow/);
	await expect(
		page.locator(".package-cell code").filter({ hasText: /^fmt$/ }),
	).toBeVisible();
	await expect(
		page.locator(".package-cell code").filter({ hasText: /^container\/list$/ }),
	).toHaveCount(0);
	await page.getByRole("button", { name: "Red" }).click();
	await expect(page).toHaveURL(/color=red/);
	await expect(
		page.locator(".package-cell code").filter({ hasText: /^container\/list$/ }),
	).toBeVisible();
	await expect(
		page.locator(".package-cell code").filter({ hasText: /^fmt$/ }),
	).toHaveCount(0);
	await page.getByRole("button", { name: "Green" }).click();
	await expect(page).toHaveURL(/color=green/);
	await expect(
		page.locator(".package-cell code").filter({ hasText: /^structs$/ }),
	).toBeVisible();
	await page.getByRole("searchbox").fill("fmt");
	await expect(page).toHaveURL(/q=fmt/);
	await page.goto("/conformance?q=fmt&color=yellow");
	await expect(page.getByRole("searchbox")).toHaveValue("fmt");
	await expect(page.getByRole("button", { name: "Yellow" })).toHaveAttribute(
		"aria-pressed",
		"true",
	);
	await page.getByRole("searchbox").fill("");
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
	await expect(page.locator(".console-section")).toHaveCount(0);
	await expect(page.locator(".editor-route")).toHaveCount(0);
	await expect(page.locator(".site-footer")).toHaveCount(0);
	await expect(page.getByText("Go source to Rust output")).toHaveCount(0);

	await page.getByRole("link", { name: "Try in Playground" }).click();
	await expect(page).toHaveURL(/\/playground$/);
	await expect(page.locator(".console-section")).toBeVisible();
	await expect(page.locator(".site-footer")).toHaveCount(0);
});
