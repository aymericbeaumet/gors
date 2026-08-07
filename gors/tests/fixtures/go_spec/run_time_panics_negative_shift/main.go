package main

func negativeLeftShift(count int) (recovered interface{}, shifted int) {
	defer func() {
		recovered = recover()
	}()
	x := 1
	shifted = x << count
	return nil, shifted
}

func negativeRightShift(count int64) (recovered interface{}, shifted int) {
	defer func() {
		recovered = recover()
	}()
	x := 1024
	shifted = x >> count
	return nil, shifted
}

func main() {
	if r, _ := negativeLeftShift(-1); r == nil {
		panic("left shift by negative run-time count did not panic")
	} else if _, ok := r.(error); !ok {
		panic("negative left shift panic value does not satisfy error")
	}

	if r, _ := negativeRightShift(-9); r == nil {
		panic("right shift by negative run-time count did not panic")
	} else if _, ok := r.(error); !ok {
		panic("negative right shift panic value does not satisfy error")
	}

	// Non-negative dynamic counts, including counts >= the operand width,
	// must not panic: there is no upper limit on the shift count.
	if recovered, sink := negativeLeftShift(200); recovered != nil {
		panic("left shift by large run-time count panicked")
	} else if sink != 0 {
		panic("shift by count >= width did not produce 0")
	}
	if recovered, sink := negativeRightShift(200); recovered != nil {
		panic("right shift by large run-time count panicked")
	} else if sink != 0 {
		panic("right shift by count >= width did not produce 0")
	}

	println("run-time-panics-negative-shift: ok")
}
