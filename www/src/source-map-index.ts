export type SourceMapping = [
	outputLine: number,
	outputColumn: number,
	goLine: number,
	goColumn: number,
	name: string,
];

export interface StructuredSourceMap {
	success: boolean;
	mappings: SourceMapping[];
}

// Bound client-side indexing so large generated programs cannot exhaust browser
// Map/source-map memory while the Rust output itself remains usable.
export const MAX_SOURCE_MAP_INDEX_MAPPINGS = 100_000;

export function extractRustTokenAt(
	lines: readonly string[],
	line: number,
	col: number,
): string | null {
	const lineText = lines[line];
	if (!lineText || col >= lineText.length) return null;

	const start = lineText[col];
	if (!start) return null;

	if (
		col + 1 < lineText.length &&
		start === "/" &&
		(lineText[col + 1] === "/" || lineText[col + 1] === "*")
	) {
		if (lineText[col + 1] === "/") return lineText.slice(col);
	}

	if (/[A-Za-z_]/.test(start)) {
		let end = col;
		while (end < lineText.length && /[A-Za-z0-9_]/.test(lineText[end])) {
			end++;
		}
		if (lineText[end] === "!") end++;
		return lineText.slice(col, end);
	}

	return start;
}

function bestMapping(
	mappings: readonly SourceMapping[] | undefined,
	column: number,
	mappingColumnIndex: 1 | 3,
): SourceMapping | null {
	if (!mappings?.length) return null;

	let best: SourceMapping | null = null;
	let bestDistance = Number.POSITIVE_INFINITY;
	let bestIsBeforeCursor = false;

	for (const mapping of mappings) {
		const mappingColumn = mapping[mappingColumnIndex];
		const distance = Math.abs(column - mappingColumn);
		const isBeforeCursor = mappingColumn <= column;
		const dominated =
			(!bestIsBeforeCursor && isBeforeCursor) ||
			(bestIsBeforeCursor === isBeforeCursor && distance < bestDistance);

		if (dominated) {
			best = mapping;
			bestDistance = distance;
			bestIsBeforeCursor = isBeforeCursor;
		}
	}

	return best;
}

export class SourceMapIndex {
	readonly success: boolean;
	private readonly rustLines: string[];
	private readonly byGoLine = new Map<number, SourceMapping[]>();
	private readonly byOutputLine = new Map<number, SourceMapping[]>();

	constructor(
		sourceMap: StructuredSourceMap | null | undefined,
		rustCode: string,
	) {
		const mappings = sourceMap?.mappings || [];
		this.success =
			sourceMap?.success === true &&
			mappings.length <= MAX_SOURCE_MAP_INDEX_MAPPINGS;
		this.rustLines = rustCode.split("\n");
		if (!this.success) return;

		for (const mapping of mappings) {
			const [outputLine, , goLine] = mapping;
			if (!this.byGoLine.has(goLine)) this.byGoLine.set(goLine, []);
			this.byGoLine.get(goLine)?.push(mapping);
			if (!this.byOutputLine.has(outputLine)) {
				this.byOutputLine.set(outputLine, []);
			}
			this.byOutputLine.get(outputLine)?.push(mapping);
		}
	}

	go_to_output(goLine: number, goColumn: number): number[] {
		const line = Math.max(0, goLine - 1);
		const column = Math.max(0, goColumn - 1);
		const mapping = bestMapping(this.byGoLine.get(line), column, 3);
		if (!mapping) return [];

		const [outputLine, outputColumn] = mapping;
		const token = extractRustTokenAt(this.rustLines, outputLine, outputColumn);
		return [
			outputLine + 1,
			outputColumn + 1,
			outputLine + 1,
			outputColumn + (token?.length || 1) + 1,
		];
	}

	output_to_go(outputLine: number, outputColumn: number): number[] {
		const line = Math.max(0, outputLine - 1);
		const column = Math.max(0, outputColumn - 1);
		const mapping = bestMapping(this.byOutputLine.get(line), column, 1);
		if (!mapping) return [];

		const [, , goLine, goColumn, name] = mapping;
		return [
			goLine + 1,
			goColumn + 1,
			goLine + 1,
			goColumn + (name?.length || 1) + 1,
		];
	}
}
