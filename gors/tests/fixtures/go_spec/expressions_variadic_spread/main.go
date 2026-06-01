package main

import "fmt"

func f(args ...int) {
	fmt.Println(len(args))
}

func main() {
	s := []int{1, 2, 3}
	f(s...)
}
