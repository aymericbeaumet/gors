package main

import "fmt"

func main() {
	n := uint(10)
	fmt.Println(fibonacciIterative(n))
	fmt.Println(fibonacciRecursive(n))
}

func fibonacciIterative(n uint) uint {
	a, b := uint(0), uint(1)

	for i, max := uint(1), n; i < max; i++ {
		a, b = b, a+b
	}

	return b
}

func fibonacciRecursive(n uint) uint {
	if n <= 1 {
		return n
	}
	return fibonacciRecursive(n-1) + fibonacciRecursive(n-2)
}
