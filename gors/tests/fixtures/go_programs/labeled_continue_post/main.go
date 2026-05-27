package main

import "fmt"

func main() {
	count := 0
outer:
	for i := 0; i < 3; i++ {
		for j := 0; j < 3; j++ {
			if j == 1 {
				continue outer
			}
			count += 10 + i
		}
		count += 100
	}
	fmt.Println(count)

	total := 0
	for i := 0; i < 4; i++ {
		if i%2 == 0 {
			continue
		}
		total += i
	}
	fmt.Println(total)
}
