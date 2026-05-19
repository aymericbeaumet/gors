package a

import (
	"fmt"
	"example/b"
)

func Run() {
	fmt.Println("entering a")
	b.Print()
	fmt.Println("leaving a")
}
