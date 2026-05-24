package main

import "fmt"

func compute() int {
	return 42
}

func main() {
	if x := compute(); x > 40 {
		fmt.Println("big:", x)
	} else {
		fmt.Println("small:", x)
	}

	if y := 10; y == 10 {
		fmt.Println("ten")
	}

	if z := 0; z == 0 {
		if w := 1; w == 1 {
			fmt.Println("nested:", z, w)
		}
	}
}
