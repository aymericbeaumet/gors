package main

import "fmt"

func main() {
	total := 0
	defer fmt.Println("defer", total == 6)
	for i := 0; i < 4; i++ {
		if i == 1 {
			continue
		}
		total += i
	}
	switch total {
	case 5:
		total++
	default:
		total = 0
	}
	values := []int{1, 2, 3}
	for _, value := range values {
		total += value
	}
	channel := make(chan int, 1)
	channel <- total
	select {
	case received := <-channel:
		total = received
	default:
		total = -1
	}
Label:
	total++
	if total < 13 {
		goto Label
	}
	go func() {}()
	fmt.Println(total)
}
