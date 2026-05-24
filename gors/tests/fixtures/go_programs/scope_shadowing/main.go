package main

import "fmt"

func main() {
	x := 10
	fmt.Println(x)
	{
		x := 20
		fmt.Println(x)
	}
	fmt.Println(x)
}
