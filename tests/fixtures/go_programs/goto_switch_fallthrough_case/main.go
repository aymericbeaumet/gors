package main

import "fmt"

func main() {
	i := 0
	switch 0 {
	case 0:
	Loop:
		fmt.Println("a", i)
		i++
		if i < 2 {
			goto Loop
		}
		fallthrough
	case 1:
		fmt.Println("b")
	}
	fmt.Println("done")
}
