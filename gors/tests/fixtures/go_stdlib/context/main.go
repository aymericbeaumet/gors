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

	// gors:stdlib-cover context::Background
	bg := context.Background()
	_, bgDeadline := bg.Deadline()
	fmt.Println("background", bgDeadline, bg.Err() == nil, bg.Value("k") == nil)

	// gors:stdlib-cover context::TODO
	todo := context.TODO()
	_, todoDeadline := todo.Deadline()
	fmt.Println("todo", todoDeadline, todo.Err() == nil, todo.Value("k") == nil)
}
