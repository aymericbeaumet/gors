package main

// Package-level declared types shadow names from the universe block.
type rune = string

func double(v int) int { return 2 * v }

func main() {
	// A short declaration's scope starts after its initializer, so this call
	// still resolves to the predeclared len function.
	len := len("word")
	println("len variable", len)

	true := 1 == 2
	if true {
		panic("shadowed true should be false")
	}
	println("shadowed true", true)

	converted := double(3)
	type int string
	var label int = "not a number"
	println("label", label, "converted", converted)

	var r rune = "text"
	println("aliased rune", r)

	{
		len := len + 1
		println("inner len", len)
	}
	println("outer len", len)
}
