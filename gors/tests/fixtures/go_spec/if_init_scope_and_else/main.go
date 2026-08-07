package main

func main() {
	x := 100
	calls := []string{}
	f := func(v int, name string) int {
		calls = append(calls, name)
		return v
	}
	if x := f(5, "first"); x > 10 {
		panic("inner x should shadow outer with 5")
	} else if y := f(x*2, "second"); y > 5 {
		if x != 5 || y != 10 {
			panic("if/else-if init values changed")
		}
	} else {
		panic("unreachable else")
	}
	if x != 100 {
		panic("outer x changed by if init shadowing")
	}
	if f(1, "third") > 100 {
		panic("unreachable then")
	} else if f(2, "fourth") < 0 {
		panic("unreachable else-if")
	}
	if f(3, "fifth") == 3 {
	} else if f(4, "sixth") == 4 {
		panic("else-if init must not run when then branch taken")
	}
	want := []string{"first", "second", "third", "fourth", "fifth"}
	if len(calls) != len(want) {
		panic("if initializer call count changed")
	}
	for i := range want {
		if calls[i] != want[i] {
			panic("if initializer evaluation order changed")
		}
	}
	if v := x / 4; v == 25 {
		x = v
	}
	if x != 25 {
		panic("if initializer result changed")
	}
	println("if-init-scope-and-else: ok")
}
