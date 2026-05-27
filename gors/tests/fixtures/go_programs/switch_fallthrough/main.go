package main

import "fmt"

func main() {
	x := 3
	switch {
	case x > 5:
		fmt.Println("big")
	case x > 0:
		fmt.Println("positive")
	case x == 0:
		fmt.Println("zero")
	default:
		fmt.Println("negative")
	}

	switch x {
	case 1:
		fmt.Println("one")
	case 2:
		fmt.Println("two")
	case 3:
		fmt.Println("three")
	default:
		fmt.Println("other")
	}

	switch {
	case true:
		fmt.Println("always")
	}

	y := 1
	switch y {
	case 1:
		fmt.Println("fall-one")
		fallthrough
	case 2:
		fmt.Println("fall-two")
	default:
		fmt.Println("fall-default")
	}

	z := 3
	switch z {
	default:
		fmt.Println("ordered-default")
	case 3:
		fmt.Println("ordered-three")
	}

	probes := 0
	probe := func(value int) int {
		probes = probes + 1
		return value
	}
	switch probe(1) {
	case probe(1):
		fmt.Println("side-first")
	case probe(1):
		fmt.Println("side-second")
	default:
		fmt.Println("side-default")
	}
	fmt.Println(probes)
}
