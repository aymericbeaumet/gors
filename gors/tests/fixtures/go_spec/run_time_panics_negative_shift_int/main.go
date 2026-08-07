package main

func negativeLeftShiftPanics(count int) (recovered interface{}, shifted int) {
	defer func() { recovered = recover() }()
	x := 1
	shifted = x << count
	return nil, shifted
}

func negativeRightShiftPanics(count int) (recovered interface{}, shifted int) {
	defer func() { recovered = recover() }()
	x := 1024
	shifted = x >> count
	return nil, shifted
}

func main() {
	if r, _ := negativeLeftShiftPanics(-1); r == nil {
		panic("left shift by negative run-time count did not panic")
	} else if _, ok := r.(error); !ok {
		panic("negative left shift panic value does not satisfy error")
	}
	if r, _ := negativeRightShiftPanics(-9); r == nil {
		panic("right shift by negative run-time count did not panic")
	} else if _, ok := r.(error); !ok {
		panic("negative right shift panic value does not satisfy error")
	}
	if r, shifted := negativeLeftShiftPanics(200); r != nil {
		panic("left shift by large run-time count panicked")
	} else if shifted != 0 {
		panic("shift by count >= width did not produce 0")
	}
	if r, shifted := negativeRightShiftPanics(200); r != nil {
		panic("right shift by large run-time count panicked")
	} else if shifted != 0 {
		panic("right shift by count >= width did not produce 0")
	}
	println("run-time-panics-negative-shift-int: ok")
}
