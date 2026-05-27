package main

import "fmt"

func main() {
	i := 0
	switch true {
	case true:
	Loop:
		fmt.Println(i)
		i++
		if i < 3 {
			goto Loop
		}
	default:
		fmt.Println("default")
	}
	fmt.Println("done")
}
