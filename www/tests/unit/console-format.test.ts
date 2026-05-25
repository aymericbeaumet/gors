import { describe, expect, it } from "vitest";
import { formatConsoleLine } from "../../src/console-format";

describe("formatConsoleLine", () => {
	it("escapes non-error output", () => {
		expect(formatConsoleLine({ type: "out", text: "<main>&" })).toBe(
			"&lt;main&gt;&amp;",
		);
	});

	it("linkifies rustc explanations in escaped errors", () => {
		expect(
			formatConsoleLine({
				type: "err",
				text: "run `rustc --explain E0308` for more information",
			}),
		).toContain("https://doc.rust-lang.org/error_codes/E0308.html");
	});
});
