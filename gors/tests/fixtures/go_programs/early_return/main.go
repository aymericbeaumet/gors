package main

import "fmt"

func abs(x int) int {
	if x < 0 {
		return -x
	}
	return x
}

func max(a int, b int) int {
	if a > b {
		return a
	}
	return b
}

func main() {
	fmt.Println(abs(-5))
	fmt.Println(abs(3))
	fmt.Println(max(10, 20))
	fmt.Println(max(30, 15))
}
