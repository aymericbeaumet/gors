package main

func f(args ...int) int {
	return len(args)
}

func inspect(args ...int) (length, capacity int, isNil bool) {
	return len(args), cap(args), args == nil
}

func mutate(args ...int) {
	if len(args) > 0 {
		args[0] = 99
	}
}

func main() {
	s := []int{1, 2, 3}
	if f(s...) != 3 {
		panic("variadic spread failed")
	}

	// Spread passes the slice unchanged: same underlying array, so the
	// callee's write is visible through the caller's slice.
	mutate(s...)
	if s[0] != 99 {
		panic("spread must pass the slice unchanged (same underlying array)")
	}

	// Spread passes the slice unchanged: capacity survives.
	spare := make([]int, 2, 5)
	length, capacity, isNil := inspect(spare...)
	if length != 2 || capacity != 5 || isNil {
		panic("spread must preserve length and capacity")
	}

	// Direct call: a new slice whose length and capacity are the number of
	// arguments bound to the parameter.
	length, capacity, isNil = inspect(7, 8)
	if length != 2 || capacity != 2 || isNil {
		panic("direct variadic call must build a fresh slice with len == cap == argc")
	}

	// Direct call: the fresh slice has a new underlying array, so callee
	// writes are invisible to the caller.
	t := []int{1, 2, 3}
	mutate(t[0], t[1], t[2])
	if t[0] != 1 {
		panic("direct variadic call must copy into a new underlying array")
	}

	// No arguments: the value passed is nil.
	length, capacity, isNil = inspect()
	if length != 0 || capacity != 0 || !isNil {
		panic("no arguments must pass nil")
	}

	// Spreading a nil slice passes it unchanged, i.e. still nil.
	var empty []int
	length, capacity, isNil = inspect(empty...)
	if length != 0 || capacity != 0 || !isNil {
		panic("nil slice spread must pass nil unchanged")
	}

	println("variadic spread semantics ok")
}
