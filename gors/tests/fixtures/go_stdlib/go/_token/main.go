package main

import (
	"fmt"
	"go/token"
)

func main() {
	fmt.Println("== token/basic ==")
	fmt.Println(token.ADD == token.ADD)
	fmt.Println(token.FUNC == token.FUNC)
	fmt.Println(token.STRING == token.STRING)
}
