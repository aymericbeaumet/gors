package main

import "fmt"

func newInt(x int) *int {
	return &x
}

func main() {
	p := newInt(42)
	fmt.Println(*p)
}
