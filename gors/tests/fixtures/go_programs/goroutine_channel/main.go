package main

import "fmt"

func sum(a int, b int, ch chan int) {
	ch <- a + b
}

func sendSum(ch chan int, nums ...int) {
	total := 0
	for _, n := range nums {
		total += n
	}
	ch <- total
}

func main() {
	ch := make(chan int, 1)
	go sum(3, 4, ch)
	result := <-ch
	fmt.Println(result)

	fn := sendSum
	go fn(ch, 1, 2, 3)
	fmt.Println(<-ch)

	values := []int{4, 5}
	go fn(ch, values...)
	fmt.Println(<-ch)

	fn(ch, 6)
	fmt.Println(<-ch)
}
