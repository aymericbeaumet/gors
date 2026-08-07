package main

func makeSliceNegativeLenPanics(n int) (recovered interface{}) {
	defer func() { recovered = recover() }()
	_ = make([]int, n)
	return nil
}

func makeSliceLenAboveCapPanics(length, capacity int) (recovered interface{}) {
	defer func() { recovered = recover() }()
	_ = make([]int, length, capacity)
	return nil
}

func makeChanNegativePanics(capacity int) (recovered interface{}) {
	defer func() { recovered = recover() }()
	_ = make(chan int, capacity)
	return nil
}

func check(name string, recovered interface{}) {
	if recovered == nil {
		panic(name + ": expected a run-time panic")
	}
	if _, isError := recovered.(error); !isError {
		panic(name + ": run-time panic value does not satisfy error")
	}
}

func main() {
	negative := -1
	small := 1
	large := 2

	check("make slice negative len", makeSliceNegativeLenPanics(negative))
	check("make slice len larger than cap", makeSliceLenAboveCapPanics(large, small))
	check("make chan negative buffer", makeChanNegativePanics(negative))

	s := make([]int, small, large)
	if len(s) != 1 || cap(s) != 2 {
		panic("make with variable sizes changed")
	}
	println("make-runtime-size-panics: ok")
}
