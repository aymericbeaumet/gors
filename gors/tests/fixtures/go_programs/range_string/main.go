package main

import "fmt"

func main() {
	s := "a\u4e16b"
	for i, r := range s {
		fmt.Println(i, r)
	}

	count := 0
	for range s {
		count++
	}
	fmt.Println("count", count)
}
