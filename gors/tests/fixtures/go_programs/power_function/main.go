package main

import "fmt"

func power(base int, exp int) int {
	result := 1
	for exp > 0 {
		if exp%2 == 1 {
			result = result * base
		}
		base = base * base
		exp = exp / 2
	}
	return result
}

func main() {
	fmt.Println(power(2, 0))
	fmt.Println(power(2, 10))
	fmt.Println(power(3, 5))
	fmt.Println(power(5, 3))
}
