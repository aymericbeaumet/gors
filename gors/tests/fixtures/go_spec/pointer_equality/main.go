package main



func main() {
	var x, y int = 7, 7
	p := &x
	q := &x
	r := &y
	var n1, n2 *int
	if p != q {
		panic("pointers to the same variable must be equal")
	}
	if p == r {
		panic("pointers to distinct variables must not be equal")
	}
	if n1 != n2 {
		panic("nil pointers must be equal")
	}
	if p == nil {
		panic("pointer to a variable must not equal nil")
	}
	*q = 9
	if x != 9 || *p != 9 {
		panic("write through an equal pointer must be visible through both")
	}
	pp := new(int)
	qq := new(int)
	if pp == qq {
		panic("new must allocate distinct variables")
	}
	println(p == q, p == r, n1 == n2, x, *p, *pp, y)
}
