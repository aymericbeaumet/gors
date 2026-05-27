package main

import "fmt"

func main() {
	values := []int{1, 2, 3}
	alias := values[1:]
	alias[0] = 9
	fmt.Println(values[1])
}
