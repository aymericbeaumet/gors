package main

import "fmt"

func sum(a int, b int, ch chan int) {
	ch <- a + b
}

func main() {
	ch := make(chan int)
	go sum(3, 4, ch)
	result := <-ch
	fmt.Println(result)
}
