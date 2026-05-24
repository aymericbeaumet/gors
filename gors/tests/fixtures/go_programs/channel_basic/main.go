package main

import "fmt"

func main() {
	ch := make(chan int)
	go func() {
		ch <- 42
	}()
	value := <-ch
	fmt.Println(value)
}
