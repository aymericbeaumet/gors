package main

import "fmt"

func main() {
	var x int = 1
	var y int = 2
	{
		var x int = 3
		fmt.Println(x)
	}
	fmt.Println(x, y)
}
