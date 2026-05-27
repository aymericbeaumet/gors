package main

import "fmt"

func main() {
	values := [3]int{2, 4, 6}
	ptr := &values

	total := 0
	for i, v := range ptr {
		fmt.Println(i, v)
		total += v
	}
	fmt.Println("total", total)

	count := 0
	for range ptr {
		count++
	}
	fmt.Println("count", count)

	for i := range ptr {
		if i == 1 {
			fmt.Println("middle", values[i])
		}
	}
}
