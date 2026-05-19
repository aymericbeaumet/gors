package main

import "fmt"

type Shape interface {
	Area() float64
	Perimeter() float64
}

type Rectangle struct {
	Width  float64
	Height float64
}

func (r Rectangle) Area() float64 {
	return r.Width * r.Height
}

func (r Rectangle) Perimeter() float64 {
	return 2.0 * (r.Width + r.Height)
}

func PrintShape(s Shape) {
	fmt.Println(s.Area())
	fmt.Println(s.Perimeter())
}

func main() {
	r := Rectangle{Width: 3.0, Height: 4.0}
	PrintShape(r)
}
