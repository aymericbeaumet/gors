package main

func main() {
	calls := []string{}
	flag := func(tag string, v bool) bool {
		calls = append(calls, tag)
		return v
	}

	andSkip := flag("a", false) && flag("skipA", true)
	orSkip := flag("c", true) || flag("skipC", false)
	andBoth := flag("e", true) && flag("f", false)
	orBoth := flag("g", false) || flag("h", true)
	grouped := flag("p", false) && flag("skipP", true) || flag("q", true)

	if andSkip || !orSkip || andBoth || !orBoth || !grouped {
		panic("logical operator results changed")
	}
	want := []string{"a", "c", "e", "f", "g", "h", "p", "q"}
	if len(calls) != len(want) {
		panic("short-circuit evaluated a skipped operand or skipped a required one")
	}
	for i := range want {
		if calls[i] != want[i] {
			panic("logical operand evaluation order changed")
		}
	}
	if !(true || flag("never", true)) {
		panic("|| with constant true left operand changed")
	}
	if false && flag("never2", true) {
		panic("&& with constant false left operand changed")
	}
	if len(calls) != len(want) {
		panic("right operand was evaluated despite decisive left operand")
	}
}
