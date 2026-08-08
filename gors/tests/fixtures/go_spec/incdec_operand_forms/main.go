package main

func main() {
	x := 5
	x--
	x--
	x++
	if x != 4 {
		panic("decrement changed")
	}
	s := []int{1, 2, 3}
	evals := 0
	f := func() int {
		evals++
		return 2
	}
	s[f()]++
	if evals != 1 || s[2] != 4 {
		panic("indexed increment evaluation changed")
	}
	m := map[string]int{}
	m["missing"]++
	m["missing"]--
	m["missing"]--
	if m["missing"] != -1 {
		panic("map incdec changed")
	}
	pointed := s[0]
	p := &pointed
	(*p)++
	var arr [3]int
	arr[1]++
	type T struct{ n int }
	t := T{}
	t.n++
	tp := &T{}
	tp.n++
	var b uint8 = 255
	b++
	var i8 int8 = -128
	i8--
	if pointed != 2 || arr[1] != 1 || t.n != 1 || tp.n != 1 || b != 0 || i8 != 127 {
		panic("incdec operand result changed")
	}
}
