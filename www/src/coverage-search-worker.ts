import {
	gostdlibCoverage,
	type GostdlibCoveragePackage,
} from "./gostdlib-coverage";

type CoverageStatusFilter = "all" | "green" | "yellow" | "red";

interface CoverageSearchRequest {
	readonly type: "search";
	readonly id: number;
	readonly query: string;
	readonly status: CoverageStatusFilter;
}

interface CoverageSearchEntry {
	readonly index: number;
	readonly color: CoverageStatusFilter;
	readonly searchText: string;
}

const worker = self as unknown as {
	onmessage: ((event: MessageEvent<CoverageSearchRequest>) => void) | null;
	postMessage(message: unknown): void;
};

function packageCoverageColor(
	item: GostdlibCoveragePackage,
): CoverageStatusFilter {
	if (item.testedSymbolCount === item.symbolCount) return "green";
	if (item.testedSymbolCount > 0) return "yellow";
	return "red";
}

function packageSearchText(
	item: GostdlibCoveragePackage,
	color: CoverageStatusFilter,
): string {
	const packageStatus =
		color === "green"
			? "green tested all tested"
			: color === "yellow"
				? "yellow partial partially tested"
				: "red none not tested untested";
	return [
		item.packagePath,
		packageStatus,
		...item.fixtures,
		...item.symbols.flatMap((symbol) => [
			symbol.name,
			symbol.kind,
			symbol.tested ? "tested" : "not tested untested",
			...symbol.fixtures,
		]),
	]
		.join(" ")
		.toLowerCase();
}

const coverageSearchIndex: readonly CoverageSearchEntry[] =
	gostdlibCoverage.map((item, index) => {
		const color = packageCoverageColor(item);
		return {
			index,
			color,
			searchText: packageSearchText(item, color),
		};
	});

const entriesByStatus: Record<
	CoverageStatusFilter,
	readonly CoverageSearchEntry[]
> = {
	all: coverageSearchIndex,
	green: coverageSearchIndex.filter((entry) => entry.color === "green"),
	yellow: coverageSearchIndex.filter((entry) => entry.color === "yellow"),
	red: coverageSearchIndex.filter((entry) => entry.color === "red"),
};

function searchPackageIndexes(
	query: string,
	status: CoverageStatusFilter,
): readonly number[] {
	const terms = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
	const candidates = entriesByStatus[status];
	if (terms.length === 0) return candidates.map((entry) => entry.index);
	return candidates
		.filter((entry) => terms.every((term) => entry.searchText.includes(term)))
		.map((entry) => entry.index);
}

worker.onmessage = ({ data }) => {
	if (data?.type !== "search") return;
	worker.postMessage({
		id: data.id,
		packageIndexes: searchPackageIndexes(data.query, data.status),
	});
};
