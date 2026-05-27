package main

import "fmt"

func main() {
	base := 10
	readOuter := func() int {
		if true {
			base := 1
			fmt.Println("inner", base)
		}
		return base + 5
	}
	fmt.Println("outer", readOuter())

	count := 0
	makeNext := func() func() int {
		return func() int {
			count++
			return count
		}
	}
	next := makeNext()
	fmt.Println("next", next())
	fmt.Println("next", next())
}
