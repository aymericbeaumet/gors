package main

func main() {
	values := []int{1, 2, 3}
	alias := values[1:]
	alias[0] = 9
	if values[1] != 9 {
		panic("slice alias write did not update source")
	}
	values[1] = 7
	if alias[0] != 7 {
		panic("source write did not update alias")
	}
	alias[1] += 3
	if values[2] != 6 {
		panic("slice alias compound assignment did not update source")
	}
}
