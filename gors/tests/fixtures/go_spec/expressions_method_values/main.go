package main

import "fmt"

type T struct {
	x int
}

func (t T) M() {
	fmt.Println(t.x)
}

func main() {
	t := T{x: 42}
	f := t.M
	f()
}
