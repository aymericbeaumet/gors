package main

import (
	"debug/plan9obj"
	"fmt"
)

func main() {
	fmt.Println("== plan9obj/basic ==")
	fmt.Println(plan9obj.Magic386)
	fmt.Println(plan9obj.MagicAMD64)
	fmt.Println(plan9obj.MagicARM)
}
