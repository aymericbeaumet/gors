package main

import "fmt"

var packageCount int = 3

func main() {
	var zero int
	var text = "value"
	short := packageCount + zero
	_ = text
	first, second := 1, 2
	first, third := second, first+second
	first, second = second, first
	pointer := &short
	*pointer = *pointer + 1
	fmt.Println(zero, short, first, second, third)
}
