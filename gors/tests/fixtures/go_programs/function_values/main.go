package main

import "function_values/mathx"

func add(a int, b int) int {
	return a + b
}

func sum(nums ...int) int {
	total := 0
	for _, n := range nums {
		total += n
	}
	return total
}

func addBase(base int, nums ...int) int {
	total := base
	for _, n := range nums {
		total += n
	}
	return total
}

func fail() {
	zero := len("")
	_ = 1 / zero
}

func main() {
	f := add
	if f(2, 3) != 5 {
		fail()
	}

	var typed func(int, int) int = add
	if typed(4, 5) != 9 {
		fail()
	}

	mul := mathx.Mul
	if mul(3, 4) != 12 {
		fail()
	}

	variadic := sum
	if variadic(1, 2, 3) != 6 {
		fail()
	}

	values := []int{4, 5}
	if variadic(values...) != 9 {
		fail()
	}

	var typedVariadic func(...int) int = sum
	if typedVariadic() != 0 {
		fail()
	}

	base := addBase
	if base(10, 1, 2) != 13 {
		fail()
	}

	if base(1, values...) != 10 {
		fail()
	}
}
