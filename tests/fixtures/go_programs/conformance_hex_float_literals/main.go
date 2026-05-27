package main

import "fmt"

func main() {
	quarter := 0x1p-2
	large := 0x2.p10
	fraction := 0x1.Fp+0
	underscored := 0X_1FFFP-16
	fmt.Println(quarter == 0.25, large == 2048.0, fraction == 1.9375)
	fmt.Println(underscored > 0.1249, underscored < 0.125)
}
