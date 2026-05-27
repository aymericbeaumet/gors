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
		fallthrough
	case 6:
		total++
	default:
		total = 0
	}
	labeled := 0
Outer:
	for x := 0; x < 3; x++ {
		for y := 0; y < 3; y++ {
			if y == 1 {
				continue Outer
			}
			labeled += x + y
		}
	}
	var dynamic any = 2
	switch value := dynamic.(type) {
	case int:
		total += value
	default:
		total = -1
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
	fmt.Println(total, labeled)
}
