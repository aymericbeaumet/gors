package main



// Package-level declarations may shadow predeclared identifiers too.
type rune = string

func double(v int) int { return 2 * v }

func main() {
	// The initializer of a short variable declaration is evaluated before
	// the new binding takes effect, so this "len" call is the builtin.
	len := len("word")
	println("len variable", len)

	// Shadow the predeclared constant true with a false value.
	true := 1 == 2
	if true {
		panic("shadowed true should be false")
	}
	println("shadowed true", true)

	// Shadow the predeclared type int with a local string type; the outer
	// builtin int stays usable via a conversion computed beforehand.
	converted := double(3)
	type int string
	var label int = "not a number"
	println("label", label, "converted", converted)

	// The package-level alias above shadows predeclared rune.
	var r rune = "text"
	println("aliased rune", r)

	{
		// Inner blocks can re-shadow, and the outer shadowing variable is
		// still visible until the inner declaration point.
		len := len + 1
		println("inner len", len)
	}
	println("outer len", len)
}
