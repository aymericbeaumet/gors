package main

import "fmt"

func isEven(n int) int {
	if n == 0 {
		return 1
	}
	return isOdd(n - 1)
}

func isOdd(n int) int {
	if n == 0 {
		return 0
	}
	return isEven(n - 1)
}

func main() {
	fmt.Println(isEven(10))
	fmt.Println(isEven(7))
	fmt.Println(isOdd(10))
	fmt.Println(isOdd(7))
}
