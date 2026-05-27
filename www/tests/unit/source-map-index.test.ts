import { describe, expect, it } from "vitest";
import {
	MAX_SOURCE_MAP_INDEX_MAPPINGS,
	SourceMapIndex,
	extractRustTokenAt,
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
});
