package main

import "fmt"

type Base struct {
	X int
	Y int
}

type Point struct {
	Base
	Label int
}

func main() {
	p := Point{
		Base:  Base{X: 10, Y: 20},
		Label: 42,
	}
	fmt.Println(p.X)
	fmt.Println(p.Y)
	fmt.Println(p.Label)
}
