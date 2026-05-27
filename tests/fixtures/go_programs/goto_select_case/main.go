package main

import "fmt"

func main() {
	i := 0
	select {
	default:
	Loop:
		fmt.Println(i)
		i++
		if i < 3 {
			goto Loop
		}
	}
	fmt.Println("done")
}
