package main

import "fmt"

func main() {
	x := 3
	switch {
	case x > 5:
		fmt.Println("big")
	case x > 0:
		fmt.Println("positive")
	case x == 0:
		fmt.Println("zero")
	default:
		fmt.Println("negative")
	}

	switch x {
	case 1:
		fmt.Println("one")
	case 2:
		fmt.Println("two")
	case 3:
		fmt.Println("three")
	default:
		fmt.Println("other")
	}

	switch {
	case true:
		fmt.Println("always")
	}
}
