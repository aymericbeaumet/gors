package main

type T struct {
	x int
}

func (t T) M() int {
	return t.x
}

func main() {
	t := T{x: 42}
	f := t.M
	if f() != 42 {
		panic("method value call failed")
	}
}
