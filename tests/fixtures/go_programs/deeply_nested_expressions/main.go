package main

import "fmt"

func add(a int, b int) int {
	return a + b
}

func mul(a int, b int) int {
	return a * b
}

func main() {
	fmt.Println(add(mul(2, 3), mul(4, 5)))
	fmt.Println(add(add(add(1, 2), 3), 4))
	fmt.Println(mul(add(1, 2), add(3, 4)))

	x := add(1, mul(2, add(3, 4)))
	fmt.Println(x)

	fmt.Println(((((1 + 2) * 3) - 4) + 5) * 6)
}
