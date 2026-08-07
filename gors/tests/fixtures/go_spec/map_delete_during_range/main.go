package main

func main() {
	// Deleting the current (already reached) entry never suppresses other
	// entries: every entry is produced exactly once and the map ends empty.
	m := map[string]int{"a": 1, "b": 2, "c": 3, "d": 4}
	sum := 0
	visits := 0
	for k, v := range m {
		visits++
		sum += v
		delete(m, k)
	}
	if visits != 4 || sum != 10 || len(m) != 0 {
		panic("deleting current entries during range changed")
	}

	// Deleting all not-yet-reached entries during the first iteration means
	// their iteration values are not produced: exactly one iteration runs,
	// regardless of (unspecified) iteration order.
	n := map[int]string{1: "x", 2: "y", 3: "z"}
	iterations := 0
	first := 0
	for k := range n {
		iterations++
		first = k
		for other := 1; other <= 3; other++ {
			if other != k {
				delete(n, other)
			}
		}
	}
	if iterations != 1 || len(n) != 1 {
		panic("deleting unreached entries during range changed")
	}
	if _, ok := n[first]; !ok {
		panic("current entry disappeared")
	}

	println("map-delete-during-range: ok")
}
