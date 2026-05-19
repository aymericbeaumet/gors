package main

import "fmt"

const (
	A = iota * 2
	B
	C
)

func main() {
	fmt.Println(A)
	fmt.Println(B)
	fmt.Println(C)
}
