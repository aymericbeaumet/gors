package main

import "fmt"

func main() {
	var prints []func()
	for i := 0; i < 5; i++ {
		prints = append(prints, func() {
			fmt.Println(i)
		})
		i++
	}

	for _, p := range prints {
		p()
	}
}
