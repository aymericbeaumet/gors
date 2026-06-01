package main

import "fmt"

func main() {
	x := 1
Label1:
	for i := 0; i < 1; i++ {
		x := 2
		fmt.Println(x)
		break Label1
	}
	fmt.Println(x)
}
