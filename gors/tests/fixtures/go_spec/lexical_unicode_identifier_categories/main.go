package main

func main() {
	αβ := 1
	_x9 := 2
	x٣ := 3
	ʰello := 4
	ǅungla := 5
	数9 := 6
	total := αβ + _x9 + x٣ + ʰello + ǅungla + 数9
	if total != 21 {
		panic("identifiers built from Unicode letters and digits changed")
	}
	println(total)
}
