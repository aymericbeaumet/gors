package main

import "fmt"

func double(x int) int {
	return x * 2
}

func triple(x int) int {
	return x * 3
}

func main() {
	fmt.Println(double(5))
	fmt.Println(triple(5))
	fmt.Println(double(3) + triple(2))
}
