package main

import "fmt"

func swap(a int, b int) (int, int) {
	return b, a
}

func divmod(a int, b int) (int, int) {
	return a / b, a % b
}

func main() {
	x, y := swap(1, 2)
	fmt.Println(x, y)

	q, r := divmod(17, 5)
	fmt.Println(q, r)
}
