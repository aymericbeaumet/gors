package main

func makeSliceNegativeLenPanics(n int) (recovered interface{}) {
	defer func() {
		recovered = recover()
	}()
	_ = make([]int, n) // n negative at run time: run-time panic
	return nil
}

func makeSliceLenAboveCapPanics(n, m int) (recovered interface{}) {
	defer func() {
		recovered = recover()
	}()
	_ = make([]int, n, m) // n larger than m at run time: run-time panic
	return nil
}

func makeChanNegativePanics(n int) (recovered interface{}) {
	defer func() {
		recovered = recover()
	}()
	_ = make(chan int, n) // n negative at run time: run-time panic
	return nil
}

func fullSliceExprOutOfRangePanics(s []int, max int) (recovered interface{}) {
	defer func() {
		recovered = recover()
	}()
	_ = s[0:1:max] // max > cap(s) at run time: run-time panic
	return nil
}

func main() {
	if r := makeSliceNegativeLenPanics(-1); r == nil {
		panic("make slice with negative run-time length did not panic")
	} else if _, ok := r.(error); !ok {
		panic("make negative length panic value does not satisfy error")
	}

	if r := makeSliceLenAboveCapPanics(3, 2); r == nil {
		panic("make slice with len > cap at run time did not panic")
	} else if _, ok := r.(error); !ok {
		panic("make len>cap panic value does not satisfy error")
	}

	if r := makeChanNegativePanics(-1); r == nil {
		panic("make chan with negative run-time capacity did not panic")
	} else if _, ok := r.(error); !ok {
		panic("make chan negative panic value does not satisfy error")
	}

	base := make([]int, 1, 4)
	if r := fullSliceExprOutOfRangePanics(base, 5); r == nil {
		panic("full slice expression with max > cap did not panic")
	} else if _, ok := r.(error); !ok {
		panic("full slice expression panic value does not satisfy error")
	}
	if fullSliceExprOutOfRangePanics(base, 4) != nil {
		panic("full slice expression with max == cap panicked")
	}

	// Valid run-time sizes succeed.
	s := make([]int, 2, 5)
	if len(s) != 2 || cap(s) != 5 {
		panic("make slice ignored run-time len/cap")
	}

	println("run-time-panics-make-and-full-slice: ok")
}
