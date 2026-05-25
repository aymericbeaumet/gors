package main

import "fmt"

type Point struct {
	X int
	Y int
}

func (p Point) String() string {
	return "Point"
}

func main() {
	p := Point{X: 1, Y: 2}
	fmt.Println(p)
}
