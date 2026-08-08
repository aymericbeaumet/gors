package main

type Meters float64

func (m Meters) Tripled() float64 { return float64(m) * 3 }
func (Meters) Unit() string       { return "m" }
func (_ Meters) Kind() string     { return "length" }

type List []int

func (l List) Sum() int {
	total := 0
	for _, v := range l {
		total += v
	}
	return total
}

type Pair[A, B any] struct {
	a A
	b B
}

func (p Pair[X, Y]) Swap() Pair[Y, X] { return Pair[Y, X]{a: p.b, b: p.a} }
func (p Pair[First, _]) First() First { return p.a }

type Counter int

func (counter *Counter) Double() { *counter *= 2 }

func main() {
	m := Meters(2)
	if m.Tripled() != 6 || m.Unit() != "m" || m.Kind() != "length" {
		panic("defined scalar receiver forms changed")
	}
	if (List{1, 2, 3}).Sum() != 6 {
		panic("defined slice receiver changed")
	}
	p := Pair[int, string]{a: 7, b: "seven"}
	s := p.Swap()
	if s.a != "seven" || s.b != 7 || p.First() != 7 {
		panic("generic receiver forms changed")
	}
	scale := Counter(3)
	(&scale).Double()
	if scale != 6 {
		panic("pointer receiver on defined scalar changed")
	}
}
