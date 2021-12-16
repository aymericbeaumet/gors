package main

import "fmt"

func main() {
	n := uint(10)
	fmt.Println(fibonacci(n))
}

func fibonacci(n uint) uint {
	a, b := uint(0), uint(1)

	for i, max := uint(1), n; i < max; i++ {
		a, b = b, a+b
	}

	return b
}
