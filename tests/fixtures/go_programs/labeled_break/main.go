package main

import "fmt"

func main() {
	found := 0
outer:
	for i := 0; i < 5; i++ {
		for j := 0; j < 5; j++ {
			if i*5+j == 13 {
				found = i*5 + j
				break outer
			}
		}
	}
	fmt.Println(found)
}
