package main

import "fmt"

func main() {
	count := 0
	next := func() int {
		count++
		return count
	}

	fmt.Println(next())
	fmt.Println(next())
	fmt.Println(count)
}
