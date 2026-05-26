package main

import "fmt"

func main() {
	var double func(int) int
	double = func(n int) int {
		return n * 2
	}
	fmt.Println(double(21))
}
