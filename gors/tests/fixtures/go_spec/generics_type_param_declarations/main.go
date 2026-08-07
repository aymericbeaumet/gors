package main

const P = 3
const C = 4

type T [P * C]int

type C2 int

type U[Q *C2,] struct{ v Q }

type V[Q interface{ *C2 }] struct{ v Q }

func drop[_ any](x int) int { return x + 1 }

func blanks[_, _ any]() string { return "ok" }

func main() {
	var t T
	println(len(t), cap(t))
	c := C2(9)
	u := U[*C2]{v: &c}
	v := V[*C2]{v: &c}
	println(*u.v, *v.v)
	println(drop[string](3), blanks[int, string]())
}
