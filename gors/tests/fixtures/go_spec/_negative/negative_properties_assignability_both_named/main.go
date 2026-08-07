package main

type A []int
type B []int

func main() {
	var a A = B{1}
	_ = a
}
