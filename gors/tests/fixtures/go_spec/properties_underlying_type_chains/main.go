package main

type (
	A1 = string
	A2 = A1
)

type (
	B1 string
	B2 B1
	B3 []B1
	B4 B3
)

func main() {
	// The underlying type of A1, A2, B1, and B2 is string.
	var b2 B2 = "chain" // untyped constant assignable via underlying type
	s := string(b2)     // conversion through the chain B2 -> B1 -> string
	b1 := B1(b2)
	var a2 A2 = s // alias of an alias of string: identical types
	println(s, string(b1), a2, len(b2))

	// The underlying type of B3 and B4 is []B1 (not []string).
	b3 := B3{"x", "y"}
	var lit []B1 = b3 // assignable: identical underlying types, []B1 is unnamed
	var b4 B4 = lit   // assignable: identical underlying types, source unnamed
	b4 = B4(b3)       // B3 <-> B4 needs an explicit conversion (both named)
	b4[0] = "z"       // b4 shares the underlying array with b3
	println(len(b4), string(b3[0]), string(b4[1]))
}
