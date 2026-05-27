package main

import "fmt"

func main() {
	values := []int{1, 2, 3, 4}
	a := values[1:3:4]
	b := values[:2:3]
	fmt.Println(len(a), cap(a))
	fmt.Println(len(b), cap(b))
}
