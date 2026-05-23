package main

import "fmt"

func main() {
	fmt.Println(2 + 3*4)
	fmt.Println((2 + 3) * 4)
	fmt.Println(10 - 2*3 + 1)
	fmt.Println(10 / 2 * 5)
	fmt.Println(17 % 5)
	fmt.Println(1 + 2*3 - 4/2 + 5%3)
	fmt.Println(-3 + 5)
	fmt.Println(-(3 + 5))

	fmt.Println(true || false && false)
	fmt.Println((true || false) && false)
	fmt.Println(!true || false)
	fmt.Println(!(true || false))
}
