/**
 * Share one successful lazy value, while allowing a later caller to retry
 * after a transient load or construction failure.
 */
export function createRetryableLazyLoader<T>(
	load: () => Promise<T>,
): () => Promise<T> {
	let admitted: Promise<T> | null = null;

	return () => {
		if (admitted) return admitted;

		const attempt = Promise.resolve().then(load);
		admitted = attempt;
		void attempt.catch(() => {
			if (admitted === attempt) admitted = null;
		});
		return attempt;
	};
}
