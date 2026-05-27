package main

import "fmt"

func Counter(start int) func() int {
	count := start
	return func() int {
		count++
		return count
	}
}

func main() {
	next := Counter(10)
	fmt.Println(next())
	fmt.Println(next())
}
