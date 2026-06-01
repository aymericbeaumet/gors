package main

import "fmt"

func main() {
	var x int = 1
	{
		var x int = 2
		fmt.Println(x)
	}
	fmt.Println(x)
}
