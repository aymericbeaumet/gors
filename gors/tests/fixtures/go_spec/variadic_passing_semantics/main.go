package main

func inspect(values ...int) (isNil bool, length int, capacity int) {
	return values == nil, len(values), cap(values)
}

func mutateFirst(values ...int) {
	if len(values) > 0 {
		values[0] = 99
	}
}

func appendOne(values ...int) []int {
	return append(values, 42)
}

func main() {
	// No actual arguments: the value passed is nil.
	noArgsNil, noArgsLen, noArgsCap := inspect()
	if !noArgsNil || noArgsLen != 0 || noArgsCap != 0 {
		panic("no-argument variadic call did not pass nil")
	}

	// A nil slice spread is passed unchanged, so the parameter is nil too.
	var nilSlice []int
	spreadNil, _, _ := inspect(nilSlice...)
	if !spreadNil {
		panic("nil slice spread did not pass nil unchanged")
	}

	// Direct calls build a new slice whose length and capacity equal the
	// number of arguments, and each call site may differ.
	twoNil, twoLen, twoCap := inspect(1, 2)
	fourNil, fourLen, fourCap := inspect(1, 2, 3, 4)
	if twoNil || twoLen != 2 || twoCap != 2 || fourNil || fourLen != 4 || fourCap != 4 {
		panic("direct variadic call slice length/capacity changed")
	}

	// A spread slice is passed unchanged: same underlying array, same capacity.
	backing := make([]int, 2, 5)
	backing[0] = 1
	backing[1] = 2
	spreadNilFlag, spreadLen, spreadCap := inspect(backing...)
	if spreadNilFlag || spreadLen != 2 || spreadCap != 5 {
		panic("slice spread did not pass the slice unchanged")
	}
	mutateFirst(backing...)
	if backing[0] != 99 {
		panic("mutation through a spread variadic parameter was not visible to the caller")
	}

	// Direct calls copy the arguments into a new underlying array.
	first := 7
	mutateFirst(first)
	if first != 7 {
		panic("direct variadic call aliased its argument")
	}

	// Appending within the spread slice's spare capacity writes into the
	// caller's backing array, because no new slice was created.
	shared := appendOne(backing[:2]...)
	if shared[2] != 42 || backing[:3][2] != 42 {
		panic("append through spread variadic did not share the backing array")
	}
	println("variadic-passing-semantics: ok")
}
