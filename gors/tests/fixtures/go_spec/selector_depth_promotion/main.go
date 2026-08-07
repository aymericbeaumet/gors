package main

type T0 struct{ x int }

func (t0 *T0) M0() string { return "M0" }

type T1 struct{ y int }

func (t1 T1) M1() string { return "M1" }

type T2 struct {
	z int
	T1
	*T0
}

func (t2 *T2) M2() string { return "M2" }

type Q *T2

type Inner struct{ n int }

type Outer struct {
	Inner
	n int
}

func main() {
	t := T2{z: 1, T1: T1{y: 2}, T0: &T0{x: 3}}
	p := &t
	var q Q = p

	println(t.z, t.y, t.x)
	println(p.z, p.y, p.x)
	println(q.x)
	println(p.M0(), p.M1(), p.M2(), t.M2())

	o := Outer{Inner: Inner{n: 10}, n: 20}
	println(o.n, o.Inner.n)
	o.n = 30
	println(o.n, o.Inner.n)

	p.x = 99
	println(t.T0.x, (*(*q).T0).x)

	t.y = 42
	println(t.T1.y, p.T1.y)
}
