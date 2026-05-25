export type ConsoleLineType = "cmd" | "out" | "err";

export interface ConsoleLine {
	type: ConsoleLineType;
	text: string;
}

const RUSTC_EXPLAIN_RE = /rustc --explain (E\d{4})/g;

export function escapeHtml(value: string): string {
	return value
		.replace(/&/g, "&amp;")
		.replace(/</g, "&lt;")
		.replace(/>/g, "&gt;");
}

export function linkifyRustErrors(html: string): string {
	return html.replace(
		RUSTC_EXPLAIN_RE,
		'<a href="https://doc.rust-lang.org/error_codes/$1.html" target="_blank" rel="noopener">rustc --explain $1</a>',
	);
}

export function formatConsoleLine(line: ConsoleLine): string {
	const html = escapeHtml(line.text);
	return line.type === "err" ? linkifyRustErrors(html) : html;
}
