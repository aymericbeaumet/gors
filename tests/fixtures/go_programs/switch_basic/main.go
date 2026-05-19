package main

import "fmt"

func grade(score int) int {
	switch {
	case score >= 90:
		return 5
	case score >= 80:
		return 4
	case score >= 70:
		return 3
	default:
		return 2
	}
}

func main() {
	fmt.Println(grade(95))
	fmt.Println(grade(85))
	fmt.Println(grade(75))
	fmt.Println(grade(65))
}
