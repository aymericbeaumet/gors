package main

func next(v int) (int, int) { return v + 1, v * 2 }

func param(x int) (*int, int) {
	before := &x
	x, doubled := next(x)
	_ = doubled
	return before, x
}

func main() {
	field1, offset := 1, 10
	p := &offset
	capture := func() int { return offset }
	field2, offset := field1+1, offset+5
	if *p != 15 {
		panic("pointer alias does not see redeclared assignment")
	}
	if capture() != 15 {
		panic("closure does not see redeclared assignment")
	}
	if field1 != 1 || field2 != 2 || offset != 15 {
		panic("redeclaration values changed")
	}
	bp, after := param(41)
	if *bp != 42 || after != 42 {
		panic("parameter redeclaration did not assign to original parameter")
	}
	println(*p, offset, field2, *bp, after)
}
