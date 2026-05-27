package main

import (
	"fmt"
	"structs"
)

type record struct {
	_  structs.HostLayout
	ID int
}

func main() {
	var r record
	r.ID = 7
	fmt.Println(r.ID)
}
