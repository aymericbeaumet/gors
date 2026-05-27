package main

import "fmt"

type Runner struct {
	Run func()
}

func main() {
	count := 0
	runner := Runner{Run: func() {
		count++
	}}
	runner.Run()
	runner.Run()
	fmt.Println(count)
}
