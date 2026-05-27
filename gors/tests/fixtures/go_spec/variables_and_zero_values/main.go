package main

import "fmt"

var packageCount int = 3

func main() {
	var zero int
	var text = "value"
	short := packageCount + zero
	_ = text
	pointer := &short
	*pointer = *pointer + 1
	fmt.Println(zero, short)
}
