package main

import "fmt"

func main() {
	fmt.Println(true && true)
	fmt.Println(true && false)
	fmt.Println(false && true)
	fmt.Println(false && false)

	fmt.Println(true || true)
	fmt.Println(true || false)
	fmt.Println(false || true)
	fmt.Println(false || false)

	fmt.Println(!true)
	fmt.Println(!false)

	fmt.Println(1 == 1)
	fmt.Println(1 != 1)
	fmt.Println(1 < 2)
	fmt.Println(2 > 1)
	fmt.Println(1 <= 1)
	fmt.Println(1 >= 2)
}
