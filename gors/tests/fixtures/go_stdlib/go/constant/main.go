package main

import (
	"fmt"
	"go/constant"
)

func main() {
	fmt.Println(constant.Int == constant.Int, constant.String == constant.Bool)
}
