package main

import "fmt"

func main() {
	x := 1
	y := 2
	fmt.Println(x, y)

	x = y
	fmt.Println(x, y)

	a := 10
	a += 5
	fmt.Println(a)
	a -= 3
	fmt.Println(a)
	a *= 2
	fmt.Println(a)
	a /= 4
	fmt.Println(a)
	a %= 3
	fmt.Println(a)
}
