import { expect, test } from "@playwright/test";

test("default showcase auto-compiles and updates after an edit", async ({
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
	await expect(consoleOutput).toContainText("$ gors emit-rust", {
		timeout: 2 * 60 * 1000,
	});
	await expect(consoleOutput).toContainText("gors transpiled", {
		timeout: 8 * 60 * 1000,
	});
	await expect(consoleOutput).not.toContainText("$ rustc -o main main.rs");
	await expect(consoleOutput).not.toContainText("$ ./main", { timeout: 1000 });

	await page.locator(".go .monaco-editor .view-lines").click();
	await page.keyboard.press("ControlOrMeta+A");
	await page.keyboard.type(
		["package main", "", "func main() {", "\tprintln(99)", "}"].join("\n"),
	);
	await expect(consoleOutput).toContainText("gors transpiled", {
		timeout: 8 * 60 * 1000,
	});
	await expect(consoleOutput).not.toContainText("$ rustc -o main main.rs");
	await expect(page.locator(".rust .monaco-editor")).not.toContainText(
		"triangular",
	);
	await expect(consoleOutput).not.toContainText("$ ./main", { timeout: 1000 });
	await expect(consoleOutput).not.toContainText("waiting for VM");
	await expect(consoleOutput).not.toContainText("VM ready in");

	expect(pageErrors).toEqual([]);
	expect(consoleErrors).toEqual([]);
});

test("Linux VM reaches ready", async ({ page }) => {
	test.skip(
		process.env.GORS_RUN_V86_E2E !== "1",
		"set GORS_RUN_V86_E2E=1 to exercise the heavyweight cold V86 boot",
	);
	test.setTimeout(10 * 60 * 1000);

	await page.goto("/playground");
	await page.getByRole("button", { name: "Linux VM" }).click();
	await expect(page.locator('.vm-status[data-state="ready"]')).toBeVisible({
		timeout: 8 * 60 * 1000,
	});
	await page.getByRole("button", { name: "Close" }).click();
});

test("conformance route exposes Go spec and standard library results", async ({
	page,
}) => {
	await page.goto("/conformance");
	await expect(
		page.getByRole("heading", { name: "Go compatibility" }),
	).toBeVisible();
	await expect(
		page.getByRole("heading", {
			name: "Go Language Specification Conformance",
		}),
	).toBeVisible();

	await page.getByRole("tab", { name: /Stdlib/ }).click();
	await expect(
		page.getByRole("heading", { name: "Go Standard Library Conformance" }),
	).toBeVisible();
	await expect(
		page.getByRole("searchbox", { name: "Filter packages" }),
	).toBeVisible();
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
