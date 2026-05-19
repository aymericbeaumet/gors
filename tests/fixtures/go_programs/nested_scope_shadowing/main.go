package main

import "fmt"

func main() {
	x := 1
	fmt.Println(x)
	{
		x := 2
		fmt.Println(x)
		{
			x := 3
			fmt.Println(x)
		}
		fmt.Println(x)
	}
	fmt.Println(x)

	y := 10
	for i := 0; i < 3; i++ {
		y := y + i
		fmt.Println(y)
	}
	fmt.Println(y)
}
