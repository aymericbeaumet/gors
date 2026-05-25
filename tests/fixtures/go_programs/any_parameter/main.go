package main

import "fmt"

func sink(x any) {}

func main() {
	sink([]int{1, 2, 3})
	sink("value")
	fmt.Println("ok")
}
