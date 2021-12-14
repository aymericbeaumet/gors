package main

import "fmt"

func main() {
	fmt.Println(fibonacci(10))
}

func fibonacci(n uint) uint {
	a, b := uint(0), uint(1)

	for i := uint(1); i < n; i++ {
		a, b = b, a+b
	}

	return b
}
