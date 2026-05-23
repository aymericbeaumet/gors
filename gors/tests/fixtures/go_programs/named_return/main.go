package main

import "fmt"

func swap(a int, b int) (x int, y int) {
	x = b
	y = a
	return
}

func main() {
	a, b := swap(1, 2)
	fmt.Println(a)
	fmt.Println(b)
}
