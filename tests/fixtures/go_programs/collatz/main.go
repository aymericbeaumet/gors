package main

import "fmt"

func collatz(n int) int {
	steps := 0
	for n != 1 {
		if n%2 == 0 {
			n = n / 2
		} else {
			n = n*3 + 1
		}
		steps++
	}
	return steps
}

func main() {
	fmt.Println(collatz(1))
	fmt.Println(collatz(6))
	fmt.Println(collatz(27))
}
