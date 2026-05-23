package main

import "fmt"

func main() {
	ch := make(chan int, 1)
	ch <- 42
	close(ch)

	val, ok := <-ch
	fmt.Println(val, ok)

	val2, ok2 := <-ch
	fmt.Println(val2, ok2)
}
