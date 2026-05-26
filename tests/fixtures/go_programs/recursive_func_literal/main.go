package main

import "fmt"

func main() {
	var fact func(int) int
	fact = func(n int) int {
		if n <= 1 {
			return 1
		}
		return n * fact(n-1)
	}
	fmt.Println(fact(5))
}
