package main

import (
	"fmt"
	"strconv"
)

func main() {
	fmt.Println(strconv.Itoa(42))
	fmt.Println(strconv.FormatBool(true))
	fmt.Println(strconv.FormatBool(false))
	fmt.Println(strconv.FormatInt(255, 16))
	fmt.Println(strconv.FormatInt(255, 2))
}
