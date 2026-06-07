package main

func main() {
	var x int = 1
	{
		var x int = 2
		if x != 2 {
			panic("inner block shadowing failed")
		}
	}
	if x != 1 {
		panic("outer block binding changed")
	}
}
