package main

func main() {
	n, changed := 1, false
	if n == 1 {
		n, inner := 100, true
		_ = inner
		n++
		if n != 101 {
			panic("inner n not independent")
		}
	}
	if n != 1 {
		panic("outer n was modified by inner-block :=")
	}
	for i := 0; i < 1; i++ {
		n, loop := 200, true
		_, _ = n, loop
	}
	if n != 1 || changed {
		panic("outer n changed after loop-body :=")
	}
	switch n, s := 300, "s"; s {
	default:
		if n != 300 {
			panic("switch init n wrong")
		}
	}
	if n != 1 {
		panic("outer n changed by switch init :=")
	}
	println(n, changed)
}
