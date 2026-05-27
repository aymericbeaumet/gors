package main

import "fmt"

func main() {
	x := 1
	switch x {
	case 1:
		fmt.Println("case-one")
		break
		fmt.Println("after-break")
	case 2:
		fmt.Println("case-two")
	}
	fmt.Println("after-switch")

outer:
	switch x {
	case 1:
		if x == 1 {
			break outer
		}
		fmt.Println("after-labeled-break")
	default:
		fmt.Println("default")
	}
	fmt.Println("after-labeled-switch")
}
