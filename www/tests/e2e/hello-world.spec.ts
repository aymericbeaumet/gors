import { expect, test } from "@playwright/test";

test.skip("default bootstrap program auto-compiles and runs manually", async ({
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
	await page.getByRole("button", { name: "Run" }).click();
	await expect(consoleOutput).toContainText("$ rustc -o main main.rs", {
		timeout: 7 * 60 * 1000,
	});
	await expect(consoleOutput).toContainText("$ ./main", {
		timeout: 9 * 60 * 1000,
	});
	await expect(consoleOutput).toContainText("10", {
		timeout: 10 * 60 * 1000,
	});
	await expect
		.poll(() =>
			consoleOutput.evaluate((node) => {
				const text = node.textContent ?? "";
				return (
					text.indexOf("$ gors emit-rust") <
						text.indexOf("$ rustc -o main main.rs") &&
					text.indexOf("$ rustc -o main main.rs") < text.indexOf("$ ./main")
				);
			}),
		)
		.toBe(true);

	await page.locator(".go .monaco-editor .view-lines").click();
	await page.keyboard.press("ControlOrMeta+A");
	await page.keyboard.type(
		["package main", "", "func main() {", "\tprintln(99)", "}"].join("\n"),
	);
	await expect(consoleOutput).toContainText("gors transpiled", {
		timeout: 8 * 60 * 1000,
	});
	await expect(consoleOutput).not.toContainText("$ rustc -o main main.rs");
	await expect(page.locator(".rust .monaco-editor")).not.toContainText("10");
	await expect(consoleOutput).not.toContainText("$ ./main", { timeout: 1000 });
	await expect(consoleOutput).not.toContainText("waiting for VM");
	await expect(consoleOutput).not.toContainText("VM ready in");

	expect(pageErrors).toEqual([]);
	expect(consoleErrors).toEqual([]);
});

test("conformance route reports the hard-cutover baseline", async ({
	page,
}) => {
	await page.goto("/conformance");
	await expect(
		page.getByRole("heading", { name: "Conformance baseline reset" }),
	).toBeVisible();
	await expect(page.getByText("No old backend or fallback")).toBeVisible();
	await expect(page.getByText("Implemented foundation")).toBeVisible();
	await expect(page.getByText("Migration backlog")).toBeVisible();
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
