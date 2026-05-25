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
	await expect(page.getByText("Ready to run")).toBeVisible({
		timeout: 8 * 60 * 1000,
	});
	await page.getByRole("button", { name: "Run" }).click();
	await expect(consoleOutput).toContainText("$ ./main", {
		timeout: 9 * 60 * 1000,
	});
	await expect(consoleOutput).toContainText("Hello, World!", {
		timeout: 10 * 60 * 1000,
	});

	expect(pageErrors).toEqual([]);
	expect(consoleErrors).toEqual([]);
});

test("coverage route shows stdlib package and symbol coverage", async ({
	page,
}) => {
	await page.goto("/coverage");

	await expect(
		page.getByRole("heading", { name: "Go stdlib coverage" }),
	).toBeVisible();
	await expect(page.getByText("51/353")).toBeVisible();
	await expect(page.getByText("294/12599")).toBeVisible();

	await page.getByRole("searchbox").fill("fmt");
	await expect(page.getByText("fmt", { exact: true })).toBeVisible();
	await expect(page.getByText("Println", { exact: true })).toBeVisible();
});
