package main

import "fmt"

func sum(label string, nums ...int) int {
	total := 0
	for _, n := range nums {
		total += n
	}
	fmt.Println(label, total)
	return total
}

func main() {
	sum("plain", 1, 2, 3)
	values := []int{4, 5}
	sum("spread", values...)
	sum("empty")
}
