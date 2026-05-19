package main

import "fmt"

func main() {
	day := 3
	switch day {
	case 1, 7:
		fmt.Println(0)
	case 2, 3, 4, 5, 6:
		fmt.Println(1)
	default:
		fmt.Println(2)
	}
}
