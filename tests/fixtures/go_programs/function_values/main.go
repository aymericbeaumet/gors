package main

import "function_values/mathx"

func add(a int, b int) int {
	return a + b
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
}
