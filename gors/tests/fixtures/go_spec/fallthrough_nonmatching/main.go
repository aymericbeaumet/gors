package main

func main() {
	evals := ""
	probe := func(name string, v int) int {
		evals += name
		return v
	}

	trail := ""
	switch x := 2; x {
	case probe("a", 1):
		trail += "one "
	case probe("b", 2):
		trail += "two "
		fallthrough
	case probe("c", 99):
		trail += "ninetynine "
		fallthrough
	default:
		trail += "default "
	}
	if evals != "ab" {
		panic("fallthrough evaluated the next case expression: " + evals)
	}
	if trail != "two ninetynine default " {
		panic("fallthrough chain through non-matching case and default changed: " + trail)
	}
	println("fallthrough-nonmatching: ok")
}
