package main

import (
	"encoding/pem"
	"fmt"
)

func main() {
	fmt.Println("== pem/basic ==")
	block := pem.Block{Type: "GORSTEST", Bytes: []byte("hello")}
	fmt.Println(block.Type)
	fmt.Println(string(block.Bytes))
}
