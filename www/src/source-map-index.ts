export type SourceMapping = [
	outputLine: number,
	outputUtf16Column: number,
	goLine: number,
	goUtf16Column: number,
	name: string,
];

export interface StructuredSourceMap {
	success: boolean;
	mappings: SourceMapping[];
}

export interface PackedSourceMap {
	success: boolean;
	/** Flat Source Map v3 positions; both column fields are UTF-16 offsets. */
	positions: Uint32Array;
	names: string[];
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

	const start = codePointAtUtf16Offset(lineText, col);
	if (!start) return null;

	if (
		col + start.length < lineText.length &&
		start.value === "/" &&
		(lineText[col + 1] === "/" || lineText[col + 1] === "*")
	) {
		if (lineText[col + 1] === "/") return lineText.slice(col);
	}

	if (isIdentifierStart(start.value)) {
		let end = col + start.length;
		while (end < lineText.length) {
			const next = codePointAtUtf16Offset(lineText, end);
			if (!next || !isIdentifierContinue(next.value)) break;
			end += next.length;
		}
		if (lineText[end] === "!") end++;
		return lineText.slice(col, end);
	}

	return start.value;
}

function codePointAtUtf16Offset(
	text: string,
	offset: number,
): { value: string; length: number } | null {
	const codeUnit = text.charCodeAt(offset);
	if (
		Number.isNaN(codeUnit) ||
		(codeUnit >= 0xdc00 &&
			codeUnit <= 0xdfff &&
			offset > 0 &&
			text.charCodeAt(offset - 1) >= 0xd800 &&
			text.charCodeAt(offset - 1) <= 0xdbff)
	) {
		return null;
	}
	const codePoint = text.codePointAt(offset);
	if (codePoint === undefined) return null;
	const value = String.fromCodePoint(codePoint);
	return { value, length: value.length };
}

function isIdentifierStart(value: string): boolean {
	return value === "_" || /^\p{ID_Start}$/u.test(value);
}

function isIdentifierContinue(value: string): boolean {
	return value === "_" || /^\p{ID_Continue}$/u.test(value);
}

function bestMappingOffset(
	offsets: readonly number[] | undefined,
	positions: Uint32Array,
	column: number,
	mappingColumnOffset: 1 | 3,
): number | null {
	if (!offsets?.length) return null;

	let best: number | null = null;
	let bestDistance = Number.POSITIVE_INFINITY;
	let bestIsBeforeCursor = false;

	for (const offset of offsets) {
		const mappingColumn = positions[offset + mappingColumnOffset];
		const distance = Math.abs(column - mappingColumn);
		const isBeforeCursor = mappingColumn <= column;
		const dominated =
			(!bestIsBeforeCursor && isBeforeCursor) ||
			(bestIsBeforeCursor === isBeforeCursor && distance < bestDistance);

		if (dominated) {
			best = offset;
			bestDistance = distance;
			bestIsBeforeCursor = isBeforeCursor;
		}
	}

	return best;
}

export class SourceMapIndex {
	readonly success: boolean;
	private readonly rustLines: string[];
	private readonly positions: Uint32Array;
	private readonly names: readonly string[];
	private readonly byGoLine = new Map<number, number[]>();
	private readonly byOutputLine = new Map<number, number[]>();

	constructor(
		sourceMap: StructuredSourceMap | PackedSourceMap | null | undefined,
		rustCode: string,
	) {
		const packed = sourceMap && "positions" in sourceMap;
		const mappings = !packed && sourceMap ? sourceMap.mappings : [];
		this.positions = packed
			? sourceMap.positions
			: Uint32Array.from(
					mappings.flatMap(([outputLine, outputColumn, goLine, goColumn]) => [
						outputLine,
						outputColumn,
						goLine,
						goColumn,
					]),
				);
		this.names = packed
			? sourceMap.names
			: mappings.map(([, , , , name]) => name);
		const mappingCount = this.positions.length / 4;
		this.success =
			sourceMap?.success === true &&
			this.positions.length % 4 === 0 &&
			this.names.length === mappingCount &&
			mappingCount <= MAX_SOURCE_MAP_INDEX_MAPPINGS;
		this.rustLines = rustCode.split("\n");
		if (!this.success) return;

		for (let offset = 0; offset < this.positions.length; offset += 4) {
			const outputLine = this.positions[offset];
			const goLine = this.positions[offset + 2];
			if (!this.byGoLine.has(goLine)) this.byGoLine.set(goLine, []);
			this.byGoLine.get(goLine)?.push(offset);
			if (!this.byOutputLine.has(outputLine)) {
				this.byOutputLine.set(outputLine, []);
			}
			this.byOutputLine.get(outputLine)?.push(offset);
		}
	}

	go_to_output(goLine: number, goColumn: number): number[] {
		const line = Math.max(0, goLine - 1);
		const column = Math.max(0, goColumn - 1);
		const offset = bestMappingOffset(
			this.byGoLine.get(line),
			this.positions,
			column,
			3,
		);
		if (offset === null) return [];

		const outputLine = this.positions[offset];
		const outputColumn = this.positions[offset + 1];
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
		const offset = bestMappingOffset(
			this.byOutputLine.get(line),
			this.positions,
			column,
			1,
		);
		if (offset === null) return [];

		const goLine = this.positions[offset + 2];
		const goColumn = this.positions[offset + 3];
		const name = this.names[offset / 4];
		return [
			goLine + 1,
			goColumn + 1,
			goLine + 1,
			goColumn + (name?.length || 1) + 1,
		];
	}
}
