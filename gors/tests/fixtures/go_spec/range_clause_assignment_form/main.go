package main

func main() {
	i := 2
	x := []int{3, 5, 7}
	for i, x[i] = range x {
		break
	}
	if i != 0 {
		panic("range assignment first iteration index changed")
	}
	if x[0] != 3 || x[1] != 5 || x[2] != 3 {
		panic("range assignment two-phase write changed")
	}

	j := 1
	y := []int{10, 20}
	for j, y[j] = range y {
	}
	if j != 1 || y[0] != 10 || y[1] != 10 {
		panic("range assignment loop changed")
	}
}
