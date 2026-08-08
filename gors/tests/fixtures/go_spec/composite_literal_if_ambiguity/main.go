package main

type T struct{ a, b, c int }

func main() {
	x := 2
	if x == (T{1, 2, 3}).b {
		println("parenthesized literal operand parsed")
	}
	if (x == T{1, 2, 3}.b) {
		println("parenthesized condition parsed")
	}
	y := [3]int{10, 20, 30}
	if y[T{0, 1, 2}.b] == 20 {
		println("literal inside index brackets needs no parentheses")
	}
	for i := (T{5, 6, 7}).a + 0; i < 6; i++ {
		println("for header with parenthesized literal:", i)
	}
	switch (T{1, 2, 3}).c {
	case 3:
		println("switch header with parenthesized literal")
	}
	if v := (T{4, 5, 6}).a + 0; v == 4 {
		println("if init statement with parenthesized literal")
	}
}
