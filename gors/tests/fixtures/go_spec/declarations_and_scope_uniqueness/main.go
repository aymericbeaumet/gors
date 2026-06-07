package main

func main() {
	var x int = 1
	var y int = 2
	{
		var x int = 3
		if x != 3 {
			panic("inner unique binding changed")
		}
	}
	if x != 1 || y != 2 {
		panic("outer unique binding changed")
	}
}
