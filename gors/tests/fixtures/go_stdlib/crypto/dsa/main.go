package main

import (
	"crypto/dsa"
	"fmt"
)

func main() {
	fmt.Println("== dsa/basic ==")
	fmt.Println(int(dsa.L1024N160))
	fmt.Println(int(dsa.L2048N224))
	fmt.Println(int(dsa.L2048N256))
	fmt.Println(int(dsa.L3072N256))
}
