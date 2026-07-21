import { describe, expect, it } from "vitest";
import {
	extractRustTokenAt,
	MAX_SOURCE_MAP_INDEX_MAPPINGS,
	type PackedSourceMap,
	SourceMapIndex,
	type SourceMapping,
	type StructuredSourceMap,
} from "../../src/source-map-index";

describe("SourceMapIndex", () => {
	it("maps Go positions to Rust token spans", () => {
		const rustCode = 'fn main() {\n    println!("Hello, World!");\n}\n';
		const sourceMap: StructuredSourceMap = {
			success: true,
			mappings: [[1, 4, 5, 1, "fmt"]],
		};

		const index = new SourceMapIndex(sourceMap, rustCode);

		expect(index.go_to_output(6, 2)).toEqual([2, 5, 2, 13]);
	});

	it("maps Rust positions back to Go token spans", () => {
		const rustCode = 'fn main() {\n    println!("Hello, World!");\n}\n';
		const sourceMap: StructuredSourceMap = {
			success: true,
			mappings: [[1, 4, 5, 1, "fmt"]],
		};

		const index = new SourceMapIndex(sourceMap, rustCode);

		expect(index.output_to_go(2, 8)).toEqual([6, 2, 6, 5]);
	});

	it("indexes packed mappings transferred from the compiler worker", () => {
		const rustCode = 'fn main() {\n    println!("Hello, World!");\n}\n';
		const sourceMap: PackedSourceMap = {
			success: true,
			positions: Uint32Array.from([1, 4, 5, 1]),
			names: ["fmt"],
		};

		const index = new SourceMapIndex(sourceMap, rustCode);

		expect(index.go_to_output(6, 2)).toEqual([2, 5, 2, 13]);
		expect(index.output_to_go(2, 8)).toEqual([6, 2, 6, 5]);
	});

	it("keeps BMP and non-BMP spans in Monaco UTF-16 columns", () => {
		const rustToken = "𐐀name!";
		const rustCode = `/* é😀 */ ${rustToken}();`;
		const rustColumn = rustCode.indexOf(rustToken);
		const goName = "é😀x";
		const goColumn = 3;
		const sourceMap: PackedSourceMap = {
			success: true,
			positions: Uint32Array.from([0, rustColumn, 0, goColumn]),
			names: [goName],
		};

		const index = new SourceMapIndex(sourceMap, rustCode);

		expect(index.go_to_output(1, goColumn + 1)).toEqual([
			1,
			rustColumn + 1,
			1,
			rustColumn + rustToken.length + 1,
		]);
		expect(index.output_to_go(1, rustColumn + 1)).toEqual([
			1,
			goColumn + 1,
			1,
			goColumn + goName.length + 1,
		]);
	});

	it("skips oversized source maps", () => {
		const sourceMap: StructuredSourceMap = {
			success: true,
			mappings: Array.from(
				{ length: MAX_SOURCE_MAP_INDEX_MAPPINGS + 1 },
				(_, index): SourceMapping => [index, 0, index, 0, "x"],
			),
		};

		const index = new SourceMapIndex(sourceMap, "x\n");

		expect(index.success).toBe(false);
		expect(index.go_to_output(1, 1)).toEqual([]);
	});
});

describe("extractRustTokenAt", () => {
	it("extracts macro calls as one token", () => {
		expect(extractRustTokenAt(['println!("ok");'], 0, 0)).toBe("println!");
	});

	it("extracts Unicode identifiers without splitting surrogate pairs", () => {
		expect(extractRustTokenAt(["éclair!();"], 0, 0)).toBe("éclair!");

		const line = "/* é😀 */ 𐐀name!();";
		const column = line.indexOf("𐐀name");
		expect(extractRustTokenAt([line], 0, column)).toBe("𐐀name!");
		expect(extractRustTokenAt([line], 0, column + 1)).toBeNull();
	});
});
