package main

var packageCount int = 3

func main() {
	var zero int
	var text = "value"
	short := packageCount + zero
	_ = text
	first, second := 1, 2
	first, third := second, first+second
	first, second = second, first
	pointer := &short
	*pointer = *pointer + 1
	if zero != 0 || short != 4 {
		panic("zero or pointer-updated value changed")
	}
	if first != 2 || second != 2 || third != 3 {
		panic("short declaration assignments changed")
	}
}
