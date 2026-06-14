package main

import "fmt"

func main() {
	ch := make(chan int, 1)
	ch <- 1
	i := 0
	select {
	case <-ch:
	Loop:
		fmt.Println("case", i)
		i++
		if i < 2 {
			goto Loop
		}
	default:
		fmt.Println("default")
	}
	fmt.Println("done")
}
