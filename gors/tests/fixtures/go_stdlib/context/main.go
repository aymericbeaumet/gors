package main

import (
	"context"
	"fmt"
)

func main() {
	// gors:stdlib-cover context::Canceled
	fmt.Println(context.Canceled.Error())
	// gors:stdlib-cover context::DeadlineExceeded
	fmt.Println(context.DeadlineExceeded.Error())
}
