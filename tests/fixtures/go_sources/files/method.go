package main

import "math"

type Point struct {
	x, y float64
}

func (p *Point) Length() float64 {
	x2 := p.x * p.x
	y2 := p.y * p.y
	return math.Sqrt(x2 + y2)
}

func (p *Point) Scale(factor float64) {
	p.x *= factor
	p.y *= factor
}
