package main

import "fmt"

func main() {
	var x any = 42
	switch label := "int"; x.(type) {
	case int:
		fmt.Println(label)
	default:
		fmt.Println("default")
	}
}
