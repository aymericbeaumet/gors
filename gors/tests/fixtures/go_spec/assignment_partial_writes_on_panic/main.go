package main

type point struct{ px, py int }

func writePastEnd(values []int) {
	defer func() {
		if recover() == nil {
			panic("expected index out of range panic")
		}
	}()
	values[1], values[3] = 4, 5
}

func writeThroughNil(values []int, p *point) {
	defer func() {
		if recover() == nil {
			panic("expected nil dereference panic")
		}
	}()
	values[2], p.px = 6, 7
}

func main() {
	x := []int{1, 2, 3}
	writePastEnd(x)
	if x[0] != 1 || x[1] != 4 || x[2] != 3 {
		panic("earlier index write was not visible after later panic")
	}

	y := []int{0, 0, 0}
	var p *point
	writeThroughNil(y, p)
	if y[0] != 0 || y[1] != 0 || y[2] != 6 {
		panic("earlier slice write was not visible after nil dereference")
	}
	println("assignment-partial-writes: ok")
}
