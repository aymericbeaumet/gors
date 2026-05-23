package main

import "fmt"

func main() {
	// delete from map
	m := map[string]int{"a": 1, "b": 2, "c": 3}
	delete(m, "b")
	fmt.Println(len(m))

	// clear map
	m2 := map[string]int{"x": 10, "y": 20}
	clear(m2)
	fmt.Println(len(m2))

	// clear slice
	s := []int{1, 2, 3, 4, 5}
	clear(s)
	fmt.Println(s)
}
