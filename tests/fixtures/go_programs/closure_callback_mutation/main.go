package main

import "fmt"

func Twice(f func()) {
	f()
	f()
}

func main() {
	count := 0
	Twice(func() {
		count++
	})
	fmt.Println(count)
}
