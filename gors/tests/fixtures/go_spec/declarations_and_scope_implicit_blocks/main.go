package main

func main() {
	x := 1
	if x := 2; x == 2 {
		println("if-then x", x)
	} else {
		println("if-else x", x)
	}
	if x := 3; x != 3 {
		panic("if implicit block binding changed")
	} else {
		// The identifier declared in the if header is visible in the else branch.
		println("else sees header x", x)
	}
	if x != 1 {
		panic("outer x changed by if implicit block")
	}
	for x := 10; x < 12; x++ {
		println("for x", x)
	}
	if x != 1 {
		panic("outer x changed by for implicit block")
	}
	switch x := 5; x {
	case 5:
		// Each clause is its own implicit block: this y is independent...
		y := "first"
		println("clause", y, x)
	default:
		// ...from this y, so the duplicate name is legal.
		y := "second"
		println("clause", y, x)
	}
	if x != 1 {
		panic("outer x changed by switch implicit block")
	}
	switch x := 7; x {
	case 7:
		x := 8 // shadows the switch-header x inside the clause block
		println("shadowed clause x", x)
	}
	println("outer x", x)
}
