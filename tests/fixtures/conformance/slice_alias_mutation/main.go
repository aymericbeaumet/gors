package main

import "fmt"

func main() {
	values := []int{1, 2, 3, 4}
	window := values[1:3]
	window[0] = 20
	fmt.Println(values[1], window[0], len(window), cap(window))

	head := values[:2]
	head[1] += 2
	fmt.Println(values[1], head[1], len(head), cap(head))
}
