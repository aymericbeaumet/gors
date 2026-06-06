package main

import (
	"context"
	"fmt"
)

type contextKey string

func main() {
	// gors:stdlib-cover context::Canceled
	fmt.Println(context.Canceled.Error())
	// gors:stdlib-cover context::DeadlineExceeded
	fmt.Println(context.DeadlineExceeded.Error())

	// gors:stdlib-cover context::Background
	// gors:stdlib-cover context::Context
	bg := context.Background()
	_, bgDeadline := bg.Deadline()
	fmt.Println("background", bgDeadline, bg.Done() == nil, bg.Err() == nil, bg.Value("k") == nil)

	// gors:stdlib-cover context::TODO
	todo := context.TODO()
	_, todoDeadline := todo.Deadline()
	fmt.Println("todo", todoDeadline, todo.Done() == nil, todo.Err() == nil, todo.Value("k") == nil)

	// gors:stdlib-cover context::WithValue
	{
		valued := context.WithValue(bg, contextKey("name"), "ada")
		fmt.Println("with-value", valued.Value(contextKey("name")), valued.Value(contextKey("missing")) == nil)
	}

	// gors:stdlib-cover context::WithCancel
	// gors:stdlib-cover context::CancelFunc
	cancelable, cancel := context.WithCancel(bg)
	_, cancelDeadline := cancelable.Deadline()
	fmt.Println("with-cancel-before", cancelDeadline, cancelable.Err() == nil, cancelable.Value(contextKey("name")) == nil)
	select {
	case <-cancelable.Done():
		fmt.Println("with-cancel-open", false)
	default:
		fmt.Println("with-cancel-open", true)
	}
	cancel()
	fmt.Println("with-cancel-after", cancelable.Err().Error())
	select {
	case <-cancelable.Done():
		fmt.Println("with-cancel-closed", true)
	default:
		fmt.Println("with-cancel-closed", false)
	}
	cancel()
	fmt.Println("with-cancel-idempotent", cancelable.Err().Error())
}
