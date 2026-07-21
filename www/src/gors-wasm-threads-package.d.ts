declare module "gors-wasm-threads-package" {
	export default function initWasm(): Promise<WebAssembly.Exports>;
	export function build_rust(input: string): unknown;
	export function compiler_thread_count(): number;
	export function export_resolver_cache(): Uint8Array;
	export function import_resolver_cache(bytes: Uint8Array): number;
	export function initThreadPool(threadCount: number): Promise<void>;
}
