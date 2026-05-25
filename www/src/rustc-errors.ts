import type * as monaco from "monaco-editor";

// eslint-disable-next-line no-control-regex
const ANSI_RE = /\x1b\[[0-9;]*m/g;

export function parseRustcErrors(
	text: string,
	markerSeverity: typeof monaco.MarkerSeverity,
): monaco.editor.IMarkerData[] {
	const markers: monaco.editor.IMarkerData[] = [];
	const clean = text.replace(ANSI_RE, "");
	const re =
		/^(error|warning)(?:\[([A-Z]\d+)\])?: (.+)\n\s*--> [^:]+:(\d+):(\d+)/gm;
	let match: RegExpExecArray | null;
	while ((match = re.exec(clean)) !== null) {
		const severity =
			match[1] === "warning" ? markerSeverity.Warning : markerSeverity.Error;
		const code = match[2] || "";
		const message = match[3];
		const line = Number.parseInt(match[4], 10);
		const col = Number.parseInt(match[5], 10);
		const after = clean.substring(
			match.index + match[0].length,
			match.index + match[0].length + 500,
		);
		const underline = after.match(/^\s*\|?\s*(\^+)/m);
		const endCol = underline ? col + underline[1].length : col + 1;
		markers.push({
			severity,
			message: code ? `${code}: ${message}` : message,
			startLineNumber: line,
			startColumn: col,
			endLineNumber: line,
			endColumn: endCol,
			source: "rustc",
			code,
		});
	}
	return markers;
}
