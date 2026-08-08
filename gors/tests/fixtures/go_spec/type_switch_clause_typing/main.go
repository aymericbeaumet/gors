package main

func describe(x any) string {
	switch v := x.(type) {
	case nil:
		if v != nil {
			panic("nil case variable must be the nil interface value")
		}
		return "nil interface"
	case int, string:
		if s, ok := v.(string); ok {
			return "string " + s
		}
		if v.(int)+1 != 8 {
			panic("multi-case variable must retain its interface type")
		}
		return "int 8"
	case *int:
		if v == nil {
			return "typed nil *int"
		}
		return "non-nil *int"
	default:
		if v != x {
			panic("default clause variable must hold the guard value with its interface type")
		}
		return "other"
	}
}

func assertEqual(got, want string) {
	if got != want {
		panic("unexpected type-switch result")
	}
}

func main() {
	var p *int
	q := 9
	assertEqual(describe(nil), "nil interface")
	assertEqual(describe(p), "typed nil *int")
	assertEqual(describe(&q), "non-nil *int")
	assertEqual(describe(7), "int 8")
	assertEqual(describe("hi"), "string hi")
	assertEqual(describe(3.5), "other")
}
