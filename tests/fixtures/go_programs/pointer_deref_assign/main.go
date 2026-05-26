package main

import "fmt"

func newInt(x int) *int {
	return &x
}

func main() {
	p := newInt(1)
	*p = 2
	(*p)++
	fmt.Println(*p)
}
