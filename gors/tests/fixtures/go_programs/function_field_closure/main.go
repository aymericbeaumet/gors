package main

import "fmt"

type Caller struct {
	F func(int) int
}

func main() {
	base := 10
	c := Caller{F: func(x int) int { return base + x }}
	d := c
	fmt.Println(c.F(1), d.F(2))
}
