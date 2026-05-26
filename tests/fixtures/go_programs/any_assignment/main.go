package main

import "fmt"

func main() {
	var x any
	x = 42
	switch v := x.(type) {
	case int:
		fmt.Println("int", v)
	default:
		fmt.Println("default")
	}

	x = "go"
	switch v := x.(type) {
	case string:
		fmt.Println("string", v)
	default:
		fmt.Println("default")
	}
}
