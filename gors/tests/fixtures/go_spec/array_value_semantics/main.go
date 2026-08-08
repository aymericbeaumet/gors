package main

func byValue(values [3]int) [3]int {
	values[0] = 0
	return values
}

func main() {
	original := [3]int{1, 2, 3}
	copied := original
	copied[0] = 9
	returned := byValue(original)
	if original[0] != 1 || copied[0] != 9 || returned[0] != 0 || returned[1] != 2 {
		panic("array copy failed")
	}
	if original == copied {
		panic("unequal arrays compare equal")
	}
	copied[0] = 1
	if original != copied {
		panic("equal arrays compare unequal")
	}
	println("array-copy: ok")
}
