package main

import "fmt"

const packageConst = 2

type Doubler interface {
	Double() int
}

type Number struct {
	Value int
}

func (n Number) Double() int {
	return n.Value * 2
}

var packageVar = Number{Value: 3}

func callDouble(value Doubler) int {
	return value.Double()
}

func recursive(value int) int {
	if value <= 1 {
		return value
	}
	return recursive(value-1) + recursive(value-2)
}

func main() {
	packageVar := packageVar
	fmt.Println(packageConst, callDouble(packageVar), recursive(5))
}
