package main

import "fmt"

func main() {
	ch := make(chan int, 1)
	ch <- 1
	select {
	case <-ch:
		fmt.Println("case")
		break
		fmt.Println("after-case-break")
	default:
		fmt.Println("default")
	}
	fmt.Println("after-select")

	select {
	default:
		if true {
			fmt.Println("default")
			break
		}
		fmt.Println("after-default-break")
	}
	fmt.Println("done")
}
