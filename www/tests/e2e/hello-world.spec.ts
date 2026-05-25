import { expect, test } from "@playwright/test";

test("default hello world transpiles, compiles, and runs", async ({ page }) => {
	const pageErrors: string[] = [];
	const consoleErrors: string[] = [];

	page.on("pageerror", (error) => pageErrors.push(error.message));
	page.on("console", (message) => {
		if (message.type() === "error") consoleErrors.push(message.text());
	});

	await page.goto("/");

	const consoleOutput = page.locator(".console-content");
	await expect(consoleOutput).toContainText("$ gors build", {
		timeout: 2 * 60 * 1000,
	});
	await expect(page.locator(".vm-status")).toHaveAttribute(
		"data-state",
		/(ready|compiling|running)/,
		{ timeout: 5 * 60 * 1000 },
	);
	await expect(consoleOutput).toContainText("$ rustc -o main main.rs", {
		timeout: 7 * 60 * 1000,
	});
	await expect(consoleOutput).toContainText("$ ./main", {
		timeout: 9 * 60 * 1000,
	});
	await expect(consoleOutput).toContainText("Hello, World!", {
		timeout: 10 * 60 * 1000,
	});

	expect(pageErrors).toEqual([]);
	expect(consoleErrors).toEqual([]);
});
