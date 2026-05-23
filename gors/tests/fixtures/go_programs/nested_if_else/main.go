package main

import "fmt"

func classify(n int) int {
	if n > 100 {
		return 3
	} else if n > 50 {
		return 2
	} else if n > 0 {
		return 1
	} else {
		return 0
	}
}

func main() {
	fmt.Println(classify(150))
	fmt.Println(classify(75))
	fmt.Println(classify(25))
	fmt.Println(classify(-5))
}
