package main

func main() {
	evals := 0
	idx := func() int {
		evals++
		return 1
	}
	s := []int{10, 20, 30}
	s[idx()] += 5
	if evals != 1 {
		panic("op-assign evaluated index expression more than once")
	}
	s[idx()] <<= 1 + 1
	if s[1] != 100 {
		panic("op-assign right operand was not parenthesized: x op= y means x = x op (y)")
	}
	if evals != 2 {
		panic("second op-assign index evaluation count changed")
	}

	keyEvals := 0
	key := func() string {
		keyEvals++
		return "k"
	}
	m := map[string]int{}
	m[key()] += 7
	if keyEvals != 1 || m["k"] != 7 {
		panic("map op-assign must evaluate key once and read zero value for missing key")
	}
	m["k"] *= 6

	v := 2
	v <<= 1 + 2
	if v != 16 {
		panic("shift-assign precedence changed")
	}

	n := 7
	n &^= 1 << 1
	if n != 5 {
		panic("bit-clear assign changed")
	}

	if len(s) != 3 || s[0] != 10 || s[1] != 100 || s[2] != 30 {
		panic("slice final values changed")
	}
	if m["k"] != 42 {
		panic("map op-assign final value changed")
	}
	if keyEvals != 1 {
		panic("constant-key op-assign must not call key function")
	}
	println(s[0], s[1], s[2], m["k"], v, n, evals, keyEvals)
}
