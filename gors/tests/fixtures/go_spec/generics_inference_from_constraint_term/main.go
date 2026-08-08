package main

func mk[P int]() P { return 42 }

func pair[A string, B any](b B) (A, B) { return "a", b }

func main() {
	v := mk()
	var typedInt int = v
	if typedInt != 42 {
		panic("constraint-only int inference changed")
	}
	a, b := pair(3.5)
	var typedString string = a
	var typedFloat float64 = b
	if typedString != "a" || typedFloat != 3.5 {
		panic("mixed constraint and argument inference changed")
	}
	println("generics-inference-from-constraint-term: ok")
}
