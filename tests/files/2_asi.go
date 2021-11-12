package main

import "fmt"

func main() {
	a := 0 // 界 un deux trois
	b := a /* 界 quatre */ /* 界 cinq */ /* 界 six */
	c := b /*
	  界 sept
	  界 huit
	  界 neuf
	*/

	fmt.Println(c)
}
